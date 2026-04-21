#![allow(dead_code)]

use alloc::vec;
use arceos_posix_api as api;
use axerrno::{AxError, LinuxError};
use axhal::arch::TrapFrame;
use axhal::mem::{VirtAddr, PAGE_SIZE_4K};
use axhal::paging::MappingFlags;
use axhal::trap::{register_trap_handler, SYSCALL};
use axtask::current;
use axtask::TaskExtRef;
use core::ffi::{c_char, c_int, c_void};
use memory_addr::{MemoryAddr, VirtAddrRange};

const SYS_IOCTL: usize = 29;
const SYS_OPENAT: usize = 56;
const SYS_CLOSE: usize = 57;
const SYS_READ: usize = 63;
const SYS_WRITE: usize = 64;
const SYS_WRITEV: usize = 66;
const SYS_EXIT: usize = 93;
const SYS_EXIT_GROUP: usize = 94;
const SYS_SET_TID_ADDRESS: usize = 96;
const SYS_MMAP: usize = 222;

const AT_FDCWD: i32 = -100;

/// Macro to generate syscall body
///
/// It will receive a function which return Result<_, LinuxError> and convert it to
/// the type which is specified by the caller.
#[macro_export]
macro_rules! syscall_body {
    ($fn: ident, $($stmt: tt)*) => {{
        #[allow(clippy::redundant_closure_call)]
        let res = (|| -> axerrno::LinuxResult<_> { $($stmt)* })();
        match res {
            Ok(_) | Err(axerrno::LinuxError::EAGAIN) => debug!(concat!(stringify!($fn), " => {:?}"),  res),
            Err(_) => info!(concat!(stringify!($fn), " => {:?}"), res),
        }
        match res {
            Ok(v) => v as _,
            Err(e) => {
                -e.code() as _
            }
        }
    }};
}

bitflags::bitflags! {
    #[derive(Debug)]
    /// permissions for sys_mmap
    ///
    /// See <https://github.com/bminor/glibc/blob/master/bits/mman.h>
    struct MmapProt: i32 {
        /// Page can be read.
        const PROT_READ = 1 << 0;
        /// Page can be written.
        const PROT_WRITE = 1 << 1;
        /// Page can be executed.
        const PROT_EXEC = 1 << 2;
    }
}

impl From<MmapProt> for MappingFlags {
    fn from(value: MmapProt) -> Self {
        let mut flags = MappingFlags::USER;
        if value.contains(MmapProt::PROT_READ) {
            flags |= MappingFlags::READ;
        }
        if value.contains(MmapProt::PROT_WRITE) {
            flags |= MappingFlags::WRITE;
        }
        if value.contains(MmapProt::PROT_EXEC) {
            flags |= MappingFlags::EXECUTE;
        }
        flags
    }
}

bitflags::bitflags! {
    #[derive(Debug)]
    /// flags for sys_mmap
    ///
    /// See <https://github.com/bminor/glibc/blob/master/bits/mman.h>
    struct MmapFlags: i32 {
        /// Share changes
        const MAP_SHARED = 1 << 0;
        /// Changes private; copy pages on write.
        const MAP_PRIVATE = 1 << 1;
        /// Map address must be exactly as requested, no matter whether it is available.
        const MAP_FIXED = 1 << 4;
        /// Don't use a file.
        const MAP_ANONYMOUS = 1 << 5;
        /// Don't check for reservations.
        const MAP_NORESERVE = 1 << 14;
        /// Allocation is for a stack.
        const MAP_STACK = 0x20000;
    }
}

#[register_trap_handler(SYSCALL)]
fn handle_syscall(tf: &TrapFrame, syscall_num: usize) -> isize {
    ax_println!("handle_syscall [{}] ...", syscall_num);
    let ret = match syscall_num {
        SYS_IOCTL => sys_ioctl(tf.arg0() as _, tf.arg1() as _, tf.arg2() as _) as _,
        SYS_SET_TID_ADDRESS => sys_set_tid_address(tf.arg0() as _),
        SYS_OPENAT => sys_openat(
            tf.arg0() as _,
            tf.arg1() as _,
            tf.arg2() as _,
            tf.arg3() as _,
        ),
        SYS_CLOSE => sys_close(tf.arg0() as _),
        SYS_READ => sys_read(tf.arg0() as _, tf.arg1() as _, tf.arg2() as _),
        SYS_WRITE => sys_write(tf.arg0() as _, tf.arg1() as _, tf.arg2() as _),
        SYS_WRITEV => sys_writev(tf.arg0() as _, tf.arg1() as _, tf.arg2() as _),
        SYS_EXIT_GROUP => {
            ax_println!("[SYS_EXIT_GROUP]: system is exiting ..");
            axtask::exit(tf.arg0() as _)
        }
        SYS_EXIT => {
            ax_println!("[SYS_EXIT]: system is exiting ..");
            axtask::exit(tf.arg0() as _)
        }
        SYS_MMAP => sys_mmap(
            tf.arg0() as _,
            tf.arg1() as _,
            tf.arg2() as _,
            tf.arg3() as _,
            tf.arg4() as _,
            tf.arg5() as _,
        ),
        _ => {
            ax_println!("Unimplemented syscall: {}", syscall_num);
            -LinuxError::ENOSYS.code() as _
        }
    };
    ret
}

#[allow(unused_variables)]
fn sys_mmap(
    addr: *mut usize,
    length: usize,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: isize,
) -> isize {
    syscall_body!(sys_mmap, {
        // 校验mmap的基础参数
        if length == 0 {
            return Err(LinuxError::EINVAL);
        }

        // 解析权限位和行为位
        let prot = MmapProt::from_bits(prot).ok_or(LinuxError::EINVAL)?;
        let mmap_flags = MmapFlags::from_bits(flags).ok_or(LinuxError::EINVAL)?;

        // MAP_SHARED和MAP_PRIVATE只能二者取其一
        let is_shared = mmap_flags.contains(MmapFlags::MAP_SHARED);
        let is_private = mmap_flags.contains(MmapFlags::MAP_PRIVATE);
        if is_shared == is_private {
            return Err(LinuxError::EINVAL);
        }

        // 文件偏移必须非负且按页对齐
        if offset < 0 || (offset as usize) & (PAGE_SIZE_4K - 1) != 0 {
            return Err(LinuxError::EINVAL);
        }

        // 将映射长度向上按页大小对齐
        let aligned_len = align_up_4k(length)?;

        let curr = current();
        let mut aspace = curr.task_ext().aspace.lock();

        // 选择映射的虚拟地址。
        // - MAP_FIXED：直接使用用户指定地址。
        // - 非MAP_FIXED：在地址空间中找空闲区域，addr作为可选hint。
        let map_start = if mmap_flags.contains(MmapFlags::MAP_FIXED) {
            let fixed_addr = VirtAddr::from(addr as usize);
            if !fixed_addr.is_aligned_4k() {
                return Err(LinuxError::EINVAL);
            }
            fixed_addr
        } else {
            let hint = if addr.is_null() {
                aspace.base()
            } else {
                VirtAddr::from(addr as usize).align_down_4k()
            };
            // 在整个地址空间中寻找空隙
            let limit = VirtAddrRange::from_start_size(aspace.base(), aspace.size());
            aspace
                .find_free_area(hint, aligned_len, limit)
                .ok_or(LinuxError::ENOMEM)?
        };

        // 创建用户映射，立即分配物理页，不用lazy策略
        let map_perm: MappingFlags = prot.into();
        aspace
            .map_alloc(map_start, aligned_len, map_perm, true)
            .map_err(axerr_to_linux)?;

        // 如果是文件映射，则把文件内容读入映射页
        // 剩下的这一段的优雅实现应该为为aspace实现新的backend
        if !mmap_flags.contains(MmapFlags::MAP_ANONYMOUS) {
            if fd < 0 {
                return Err(LinuxError::EBADF);
            }

            let file = api::get_file_like(fd)?;
            let mut data = vec![0u8; length];
            let mut read_len = 0usize;

            while read_len < length {
                // 将文件内容读到缓冲区
                let n = file.read(&mut data[read_len..])?;
                if n == 0 {
                    break;
                }
                read_len += n;
            }

            if read_len > 0 {
                aspace
                    .write(map_start, &data[..read_len])
                    .map_err(axerr_to_linux)?;
            }
        }

        // 将映射起始地址返回给用户态
        Ok(map_start.as_usize())
    })
}

// 将长度向上对齐到4 KiB，并处理溢出
fn align_up_4k(size: usize) -> Result<usize, LinuxError> {
    size.checked_add(PAGE_SIZE_4K - 1)
        .map(|x| x & !(PAGE_SIZE_4K - 1))
        .ok_or(LinuxError::EINVAL)
}

// 将内部AxError转换为Linux errno，供系统调用返回
fn axerr_to_linux(err: AxError) -> LinuxError {
    match err {
        AxError::NoMemory => LinuxError::ENOMEM,
        _ => LinuxError::EINVAL,
    }
}

fn sys_openat(dfd: c_int, fname: *const c_char, flags: c_int, mode: api::ctypes::mode_t) -> isize {
    assert_eq!(dfd, AT_FDCWD);
    api::sys_open(fname, flags, mode) as isize
}

fn sys_close(fd: i32) -> isize {
    api::sys_close(fd) as isize
}

fn sys_read(fd: i32, buf: *mut c_void, count: usize) -> isize {
    api::sys_read(fd, buf, count)
}

fn sys_write(fd: i32, buf: *const c_void, count: usize) -> isize {
    api::sys_write(fd, buf, count)
}

fn sys_writev(fd: i32, iov: *const api::ctypes::iovec, iocnt: i32) -> isize {
    unsafe { api::sys_writev(fd, iov, iocnt) }
}

fn sys_set_tid_address(tid_ptd: *const i32) -> isize {
    let curr = current();
    curr.task_ext().set_clear_child_tid(tid_ptd as _);
    curr.id().as_u64() as isize
}

fn sys_ioctl(_fd: i32, _op: usize, _argp: *mut c_void) -> i32 {
    ax_println!("Ignore SYS_IOCTL");
    0
}
