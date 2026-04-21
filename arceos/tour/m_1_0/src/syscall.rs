#![allow(dead_code)]

use axerrno::LinuxError;
use axhal::arch::TrapFrame;
use axhal::mem::VirtAddr;
use axhal::paging::MappingFlags;
use axhal::trap::{register_trap_handler, PAGE_FAULT, SYSCALL};
use axtask::{self, TaskExtRef};

const SYS_EXIT: usize = 93;

#[register_trap_handler(SYSCALL)]
fn handle_syscall(tf: &TrapFrame, syscall_num: usize) -> isize {
    ax_println!("handle_syscall ...");
    let ret = match syscall_num {
        SYS_EXIT => {
            ax_println!("[SYS_EXIT]: process is exiting ..");
            axtask::exit(tf.arg0() as _)
        }
        _ => {
            ax_println!("Unimplemented syscall: {}", syscall_num);
            -LinuxError::ENOSYS.code() as _
        }
    };
    ret
}

#[register_trap_handler(PAGE_FAULT)]
fn handle_page_fault(va: VirtAddr, flags: MappingFlags, is_user: bool) -> bool {
    ax_println!("handle_page_fault ...");
    ax_println!("Hi there, Let me help you");

    if !is_user {
        return false;
    }

    let is_successfully = axtask::current()
        .task_ext()
        .aspace
        .lock()
        .handle_page_fault(va, flags);

    if !is_successfully {
        ax_println!(
            "Page fault cannot be handled! va={:#x?}, flags={:?}",
            va,
            flags
        );
    } else {
        ax_println!("{}: handle page fault OK!", axtask::current().id_name());
    }

    is_successfully
}
