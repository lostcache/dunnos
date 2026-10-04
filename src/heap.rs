use core::alloc::{GlobalAlloc, Layout};
use core::cell::SyncUnsafeCell;

use crate::{mm, utils};

const HEAP_SIZE: usize = utils::mb(1);
const REGION_META_SIZE: usize = size_of::<HeapRegion>();
const MIN_CAPACITY: usize = 16;
const MAX_ALIGN: usize = 16;

#[repr(C)]
struct HeapRegion {
    capacity: usize,
    next_free: *mut HeapRegion,
}

struct Heap {
    first_free: SyncUnsafeCell<*mut HeapRegion>,
    is_init: SyncUnsafeCell<bool>,
}

// One hart runs this allocator.
unsafe impl Sync for Heap {}

#[global_allocator]
static ALLOCATOR: Heap = Heap {
    first_free: SyncUnsafeCell::new(core::ptr::null_mut()),
    is_init: SyncUnsafeCell::new(false),
};

/// Reserves one contiguous heap span, then starts the free list.
pub(crate) fn init() -> Result<(), mm::MMError> {
    let start_addr = mm::usable_ranges()
        .iter()
        .find_map(|range| {
            let aligned = range.start.next_multiple_of(mm::PAGE_SIZE);
            (aligned.checked_add(HEAP_SIZE)? <= range.end).then_some(aligned)
        })
        .ok_or(mm::MMError::Init)?;

    mm::mark_used(start_addr, start_addr + HEAP_SIZE)?;

    let first_free = start_addr as *mut HeapRegion;
    unsafe {
        (*first_free).capacity = HEAP_SIZE - REGION_META_SIZE;
        (*first_free).next_free = core::ptr::null_mut();
        *ALLOCATOR.first_free.get() = first_free;
        *ALLOCATOR.is_init.get() = true;
    }

    Ok(())
}

pub(crate) fn ready() -> bool {
    unsafe { *ALLOCATOR.is_init.get() }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > MAX_ALIGN {
            return core::ptr::null_mut();
        }

        let needed = layout.size().max(MIN_CAPACITY).next_multiple_of(MAX_ALIGN);

        let mut current = unsafe { *ALLOCATOR.first_free.get() };
        let mut prev: *mut HeapRegion = core::ptr::null_mut();

        while !current.is_null() {
            let curr_cap = unsafe { (*current).capacity };
            let next_free = unsafe { (*current).next_free };

            if curr_cap >= needed {
                let replacement = if curr_cap - needed >= REGION_META_SIZE + MIN_CAPACITY {
                    let leftover =
                        (current as usize + REGION_META_SIZE + needed) as *mut HeapRegion;
                    unsafe {
                        (*leftover).capacity = curr_cap - needed - REGION_META_SIZE;
                        (*leftover).next_free = next_free;
                        (*current).capacity = needed;
                    }
                    leftover
                } else {
                    next_free
                };

                if prev.is_null() {
                    unsafe { *ALLOCATOR.first_free.get() = replacement };
                } else {
                    unsafe { (*prev).next_free = replacement };
                }

                return (current as usize + REGION_META_SIZE) as *mut u8;
            }

            prev = current;
            current = next_free;
        }

        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // The region holds the granted size. The layout can differ.
        let region = (ptr as usize - REGION_META_SIZE) as *mut HeapRegion;
        unsafe {
            (*region).next_free = *ALLOCATOR.first_free.get();
            *ALLOCATOR.first_free.get() = region;
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { core::ptr::write_bytes(ptr, 0, layout.size()) };
        }
        ptr
    }
}
