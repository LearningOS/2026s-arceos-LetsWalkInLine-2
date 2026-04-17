#![no_std]

use allocator::{AllocError, AllocResult, BaseAllocator, ByteAllocator, PageAllocator};
use core::alloc::Layout;
use core::ptr::NonNull;

/// Early memory allocator
/// Use it before formal bytes-allocator and pages-allocator can work!
/// This is a double-end memory range:
/// - Alloc bytes forward
/// - Alloc pages backward
///
/// [ bytes-used | avail-area | pages-used ]
/// |            | -->    <-- |            |
/// start       b_pos        p_pos       end
///
/// For bytes area, 'count' records number of allocations.
/// When it goes down to ZERO, free bytes-used area.
/// For pages area, it will never be freed!
///
pub struct EarlyAllocator<const SIZE: usize> {
    start: usize,
    end: usize,
    p_pos: usize,
    b_pos: usize,
    b_count: usize, // 字节端分配的次数（也即调用allc的次数）
    p_count: usize, // 页端分配的页数（也即调用alloc_pages的页数）
}

impl<const SIZE: usize> EarlyAllocator<SIZE> {
    pub const fn new() -> Self {
        Self {
            start: 0,
            end: 0,
            p_pos: 0,
            b_pos: 0,
            b_count: 0,
            p_count: 0,
        }
    }
}

#[inline]
fn align_up(pos: usize, align: usize) -> Option<usize> {
    pos.checked_add(align - 1).map(|v| v & !(align - 1))
}

#[inline]
fn align_down(pos: usize, align: usize) -> usize {
    pos & !(align - 1)
}

impl<const SIZE: usize> BaseAllocator for EarlyAllocator<SIZE> {
    fn init(&mut self, start: usize, size: usize) {
        assert!(SIZE.is_power_of_two());
        assert!(size > 0);
        let end = start.checked_add(size).expect("overflow in init range");

        self.start = start;
        self.end = end;
        self.p_pos = end;
        self.b_pos = start;
        self.b_count = 0;
        self.p_count = 0;
    }

    fn add_memory(&mut self, start: usize, size: usize) -> AllocResult {
        if size == 0 {
            return Err(AllocError::InvalidParam);
        }
        let end = start.checked_add(size).ok_or(AllocError::InvalidParam)?;

        // 如果还没有初始化过，直接初始化
        if self.end == 0 && self.start == 0 && self.p_pos == 0 && self.b_pos == 0 {
            self.init(start, size);
            return Ok(());
        }

        // 如果新内存区域在现有区域的左侧，且不重叠
        if end <= self.start {
            if end != self.start || self.b_count != 0 {
                return Err(AllocError::NoMemory);
            }
            self.start = start;
            self.b_pos = start;
            return Ok(());
        }

        // 如果新内存区域在现有区域的右侧，且不重叠
        if start >= self.end {
            if start != self.end || self.p_count != 0 {
                return Err(AllocError::NoMemory);
            }
            self.end = end;
            self.p_pos = end;
            return Ok(());
        }

        Err(AllocError::MemoryOverlap)
    }
}

impl<const SIZE: usize> ByteAllocator for EarlyAllocator<SIZE> {
    fn alloc(&mut self, layout: Layout) -> AllocResult<NonNull<u8>> {
        let alloc_start = align_up(self.b_pos, layout.align()).ok_or(AllocError::NoMemory)?;
        let alloc_end = alloc_start
            .checked_add(layout.size())
            .ok_or(AllocError::NoMemory)?;

        if alloc_end <= self.p_pos {
            self.b_pos = alloc_end;
            self.b_count = self.b_count.checked_add(1).ok_or(AllocError::NoMemory)?;
            Ok(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
        } else {
            Err(AllocError::NoMemory)
        }
    }

    fn dealloc(&mut self, _pos: NonNull<u8>, _layout: Layout) {
        assert!(self.b_count > 0);
        self.b_count -= 1;
        if self.b_count == 0 {
            self.b_pos = self.start;
        }
    }

    fn total_bytes(&self) -> usize {
        self.p_pos.saturating_sub(self.start)
    }

    fn used_bytes(&self) -> usize {
        if self.b_count > 0 {
            self.b_pos.saturating_sub(self.start)
        } else {
            0
        }
    }

    fn available_bytes(&self) -> usize {
        self.p_pos.saturating_sub(self.b_pos)
    }
}

impl<const SIZE: usize> PageAllocator for EarlyAllocator<SIZE> {
    const PAGE_SIZE: usize = SIZE;

    fn alloc_pages(&mut self, num_pages: usize, align_pow2: usize) -> AllocResult<usize> {
        if num_pages == 0 || align_pow2 == 0 {
            return Err(AllocError::InvalidParam);
        }

        let align = align_pow2.max(Self::PAGE_SIZE);
        if !align.is_power_of_two() {
            return Err(AllocError::InvalidParam);
        }

        let alloc_size = num_pages
            .checked_mul(Self::PAGE_SIZE)
            .ok_or(AllocError::InvalidParam)?;
        let range_start = self
            .p_pos
            .checked_sub(alloc_size)
            .ok_or(AllocError::NoMemory)?;
        let alloc_start = align_down(range_start, align);

        if alloc_start >= self.b_pos {
            self.p_count = self
                .p_count
                .checked_add(num_pages)
                .ok_or(AllocError::NoMemory)?;
            self.p_pos = alloc_start;
            Ok(alloc_start)
        } else {
            Err(AllocError::NoMemory)
        }
    }

    fn dealloc_pages(&mut self, _pos: usize, _num_pages: usize) {
        // no-op: pages are never freed in this early allocator
    }

    fn total_pages(&self) -> usize {
        self.p_count + self.available_pages()
    }

    fn used_pages(&self) -> usize {
        self.p_count
    }

    fn available_pages(&self) -> usize {
        self.p_pos.saturating_sub(self.b_pos) / Self::PAGE_SIZE
    }
}
