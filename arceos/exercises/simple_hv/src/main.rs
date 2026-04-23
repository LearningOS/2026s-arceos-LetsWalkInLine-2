#![cfg_attr(feature = "axstd", no_std)]
#![cfg_attr(feature = "axstd", no_main)]
#![feature(asm_const)]
#![feature(riscv_ext_intrinsics)]

extern crate alloc;
#[cfg(feature = "axstd")]
extern crate axstd as std;
#[macro_use]
extern crate axlog;

mod csrs;
mod loader;
mod regs;
mod sbi;
mod task;
mod vcpu;

use crate::regs::GprIndex;
use crate::regs::GprIndex::{A0, A1};
use axhal::mem::PhysAddr;
use csrs::defs::hstatus;
use csrs::{RiscvCsrTrait, CSR};
use loader::load_vm_image;
use riscv::register::{scause, sstatus, stval};
use sbi::SbiMessage;
use tock_registers::LocalRegisterCopy;
use vcpu::VmCpuRegisters;
use vcpu::_run_guest;

const VM_ENTRY: usize = 0x8020_0000;
const EMULATED_ILLEGAL_RET: usize = 0x1234;
const EMULATED_LOAD_RET: usize = 0x6688;
const EXPECTED_FAULT_LOAD_ADDR: usize = 0x40;

#[inline]
fn read_htval() -> usize {
    let val: usize;
    unsafe {
        core::arch::asm!("csrr {0}, 0x643", out(reg) val);
    }
    val
}

#[inline]
fn read_htinst() -> usize {
    let val: usize;
    unsafe {
        core::arch::asm!("csrr {0}, 0x64a", out(reg) val);
    }
    val
}

#[inline]
fn inst_len(insn_bits: usize) -> usize {
    //RISC-V 编码约定：最低 2 位不等于 11 => 16-bit 压缩指令；
    // 等于 11 => 至少 32-bit
    if insn_bits & 0b11 == 0b11 {
        4
    } else {
        2
    }
}

#[inline]
fn decode_csrr_mhartid(insn_bits: usize) -> Option<GprIndex> {
    // 对 csrr a1, mhartid，汇编伪指令会展开成 csrrs a1, mhartid, x0
    // 按 RISC-V 指令字段做位切片：
    // opcode = insn[6:0]（SYSTEM = 0x73）
    // rd = insn[11:7]
    // funct3 = insn[14:12]（CSRRS = 010）
    // rs1 = insn[19:15]（这里应是 x0）
    // csr = insn[31:20]（mhartid = 0xF14）
    let opcode = insn_bits & 0x7f;
    let rd = ((insn_bits >> 7) & 0x1f) as u32;
    let funct3 = (insn_bits >> 12) & 0x7;
    let rs1 = (insn_bits >> 15) & 0x1f;
    let csr = (insn_bits >> 20) & 0xfff;

    // 全部匹配才认定这是要仿真的那条指令，然后把值写入 rd
    if opcode == 0x73 && funct3 == 0b010 && rs1 == 0 && csr == 0xF14 {
        GprIndex::from_raw(rd)
    } else {
        None
    }
}

#[cfg_attr(feature = "axstd", no_mangle)]
fn main() {
    ax_println!("Hypervisor ...");

    // A new address space for vm.
    let mut uspace = axmm::new_user_aspace().unwrap();

    // Load vm binary file into address space.
    if let Err(e) = load_vm_image("/sbin/skernel2", &mut uspace) {
        panic!("Cannot load app! {:?}", e);
    }

    // Setup context to prepare to enter guest mode.
    let mut ctx = VmCpuRegisters::default();
    prepare_guest_context(&mut ctx);

    // Setup pagetable for 2nd address mapping.
    let ept_root = uspace.page_table_root();
    prepare_vm_pgtable(ept_root);

    // Kick off vm and wait for it to exit.
    while !run_guest(&mut ctx) {}

    panic!("Hypervisor ok!");
}

fn prepare_vm_pgtable(ept_root: PhysAddr) {
    let hgatp = 8usize << 60 | usize::from(ept_root) >> 12;
    unsafe {
        core::arch::asm!(
            "csrw hgatp, {hgatp}",
            hgatp = in(reg) hgatp,
        );
        core::arch::riscv64::hfence_gvma_all();
    }
}

fn run_guest(ctx: &mut VmCpuRegisters) -> bool {
    unsafe {
        _run_guest(ctx);
    }

    vmexit_handler(ctx)
}

#[allow(unreachable_code)]
fn vmexit_handler(ctx: &mut VmCpuRegisters) -> bool {
    use scause::{Exception, Trap};

    let scause = scause::read();
    match scause.cause() {
        Trap::Exception(Exception::VirtualSupervisorEnvCall) => {
            let sbi_msg = SbiMessage::from_regs(ctx.guest_regs.gprs.a_regs()).ok();
            ax_println!("VmExit Reason: VSuperEcall: {:?}", sbi_msg);
            if let Some(msg) = sbi_msg {
                match msg {
                    SbiMessage::Reset(_) => {
                        let a0 = ctx.guest_regs.gprs.reg(A0);
                        let a1 = ctx.guest_regs.gprs.reg(A1);
                        ax_println!("a0 = {:#x}, a1 = {:#x}", a0, a1);
                        assert_eq!(a0, 0x6688);
                        assert_eq!(a1, 0x1234);
                        ax_println!("Shutdown vm normally!");
                        return true;
                    }
                    _ => todo!(),
                }
            } else {
                panic!("bad sbi message! ");
            }
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            let bad_insn = stval::read();
            ax_println!(
                "Bad instruction: {:#x} sepc: {:#x}",
                bad_insn,
                ctx.guest_regs.sepc
            );

            if let Some(rd) = decode_csrr_mhartid(bad_insn) {
                ctx.guest_regs.gprs.set_reg(rd, EMULATED_ILLEGAL_RET);
                ctx.guest_regs.sepc += inst_len(bad_insn);
            } else {
                panic!(
                    "Unhandled illegal instruction: stval={:#x}, sepc={:#x}",
                    bad_insn, ctx.guest_regs.sepc
                );
            }
        }
        Trap::Exception(Exception::LoadGuestPageFault) => {
            let fault_addr = stval::read();
            let htval = read_htval();
            let htinst = read_htinst();
            ax_println!(
                "LoadGuestPageFault: stval={:#x} htval={:#x} htinst={:#x} sepc={:#x}",
                fault_addr,
                htval,
                htinst,
                ctx.guest_regs.sepc
            );

            // 目前就暂时不管二次分配的事情
            if fault_addr == EXPECTED_FAULT_LOAD_ADDR {
                ctx.guest_regs.gprs.set_reg(A0, EMULATED_LOAD_RET);
                ctx.guest_regs.sepc += 4;
            } else {
                panic!(
                    "Unhandled LoadGuestPageFault: stval={:#x}, htval={:#x}, htinst={:#x}, sepc={:#x}",
                    fault_addr,
                    htval,
                    htinst,
                    ctx.guest_regs.sepc
                );
            }
        }
        _ => {
            panic!(
                "Unhandled trap: {:?}, sepc: {:#x}, stval: {:#x}",
                scause.cause(),
                ctx.guest_regs.sepc,
                stval::read()
            );
        }
    }
    false
}

fn prepare_guest_context(ctx: &mut VmCpuRegisters) {
    // Set hstatus
    let mut hstatus =
        LocalRegisterCopy::<usize, hstatus::Register>::new(riscv::register::hstatus::read().bits());
    // Set Guest bit in order to return to guest mode.
    hstatus.modify(hstatus::spv::Guest);
    // Set SPVP bit in order to accessing VS-mode memory from HS-mode.
    hstatus.modify(hstatus::spvp::Supervisor);
    CSR.hstatus.write_value(hstatus.get());
    ctx.guest_regs.hstatus = hstatus.get();

    // Set sstatus in guest mode.
    let mut sstatus = sstatus::read();
    sstatus.set_spp(sstatus::SPP::Supervisor);
    ctx.guest_regs.sstatus = sstatus.bits();
    // Return to entry to start vm.
    ctx.guest_regs.sepc = VM_ENTRY;
}
