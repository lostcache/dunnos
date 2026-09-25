// 8. `mm.rs`: APIs `init()`, `alloc_frame()`, `free_frame()`. Zero every allocated frame (page tables need zeroed memory). */
use crate::fdt;
use core::cell::SyncUnsafeCell;

pub(crate) const PAGE_SIZE: usize = 4096;
const BITS_PER_WORD: usize = 64;

const MAX_USABLE_RANGES: usize = 16;

pub(crate) enum MMError {
    InitError,
    AllocFrameError,
    FreeFrameError,
}

/// Half-open physical range [start, end).
#[derive(Clone, Copy)]
pub(crate) struct Range {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

struct UsableRanges {
    ranges: [Range; MAX_USABLE_RANGES],
    len: usize,
}

impl UsableRanges {
    const fn new() -> Self {
        Self {
            ranges: [Range { start: 0, end: 0 }; MAX_USABLE_RANGES],
            len: 0,
        }
    }

    /// Appends [start, end) to the list.
    fn push(&mut self, r: Range) -> Result<(), MMError> {
        if self.len == MAX_USABLE_RANGES {
            return Err(MMError::InitError);
        }
        self.ranges[self.len] = r;
        self.len += 1;
        Ok(())
    }

    /// Removes [hole_start, hole_end) from every range.
    fn subtract(&mut self, hole_start: usize, hole_end: usize) -> Result<(), MMError> {
        if hole_start >= hole_end {
            return Ok(());
        }
        let mut out = Self::new();
        for r in &self.ranges[..self.len] {
            if hole_end <= r.start || hole_start >= r.end {
                out.push(*r)?;
                continue;
            }
            if r.start < hole_start {
                out.push(Range {
                    start: r.start,
                    end: hole_start,
                })?;
            }
            if hole_end < r.end {
                out.push(Range {
                    start: hole_end,
                    end: r.end,
                })?;
            }
        }
        *self = out;
        Ok(())
    }

    fn get_ranges(&self) -> &[Range] {
        &self.ranges[..self.len]
    }
}

static USABLE: SyncUnsafeCell<UsableRanges> = SyncUnsafeCell::new(UsableRanges::new());

fn get_usable() -> &'static mut UsableRanges {
    unsafe { &mut *USABLE.get() }
}

/// Physical RAM ranges that hold neither the DTB nor the kernel image.
pub(crate) fn usable_ranges() -> &'static [Range] {
    let u = get_usable();
    &u.ranges[..u.len]
}

unsafe extern "C" {
    #[link_name = "_entry"]
    static KERNEL_START: u8;
    #[link_name = "end"]
    static KERNEL_END: u8;
}

fn kernel_image_range() -> (usize, usize) {
    let start = core::ptr::addr_of!(KERNEL_START) as usize;
    let end_addr = (core::ptr::addr_of!(KERNEL_END) as usize).next_multiple_of(PAGE_SIZE);
    (start, end_addr)
}

/// One bit per 4 KiB frame. Bit `frame % 64` of `words[frame / 64]`.
struct Bitmap {
    base: usize,
    words: &'static mut [u64],
    frames: usize,
}

static BITMAP: SyncUnsafeCell<Bitmap> = SyncUnsafeCell::new(Bitmap {
    base: 0,
    words: &mut [],
    frames: 0,
});

fn get_mut_bitmap() -> &'static mut Bitmap {
    unsafe { &mut *BITMAP.get() }
}

impl Bitmap {
    fn locate(frame: usize) -> (usize, u64) {
        (frame / 64, 1 << (frame % 64))
    }

    /// Sets or clears every frame fully inside physical range
    /// [start, end). Frames outside the tracked span are ignored.
    fn set_range(&mut self, start: usize, end: usize, used: bool) {
        let first = start.saturating_sub(self.base).div_ceil(PAGE_SIZE);
        let last = (end.saturating_sub(self.base) / PAGE_SIZE).min(self.frames);
        for frame in first..last {
            let (word, mask) = Self::locate(frame);
            if used {
                self.words[word] |= mask;
            } else {
                self.words[word] &= !mask;
            }
        }
    }

    /// Physical address of the first free frame, or None when the
    /// tracked span is full. Marks the frame used on success.
    fn alloc_frame_and_return_addr(&mut self) -> Option<usize> {
        let (word_idx, word) = self
            .words
            .iter()
            .enumerate()
            .find(|(_, word)| **word != u64::MAX)?;
        let bit_idx = word.trailing_ones() as usize;
        let frame = word_idx * BITS_PER_WORD + bit_idx;
        if frame >= self.frames {
            return None;
        }
        self.words[word_idx] |= 1u64 << bit_idx;
        Some(self.base + frame * PAGE_SIZE)
    }

    fn free_frame(&mut self, addr: usize) -> Result<(), MMError> {
        if addr % PAGE_SIZE != 0 || addr < self.base {
            return Err(MMError::FreeFrameError);
        }
        let frame = (addr - self.base) / PAGE_SIZE;
        if frame >= self.frames {
            return Err(MMError::FreeFrameError);
        }
        let (word_idx, bit_idx) = Self::locate(frame);
        if self.words[word_idx] & bit_idx == 0 {
            return Err(MMError::FreeFrameError);
        }
        self.set_range(addr, addr + PAGE_SIZE, false);
        Ok(())
    }
}

pub(crate) fn init() -> Result<(), MMError> {
    let Some(node_id) = fdt::get_node_idx_by_prop_name_and_val(b"device_type", b"memory") else {
        return Err(MMError::InitError);
    };

    let mut usable = UsableRanges::new();

    let mut resource_idx = 0;
    while let Some(r) = fdt::get_resource(node_id, resource_idx) {
        let end = r.base.checked_add(r.size).ok_or(MMError::InitError)?;
        if r.size != 0 {
            usable.push(Range {
                start: r.base as usize,
                end: end as usize,
            })?;
        }
        resource_idx += 1;
    }
    if usable.len == 0 {
        return Err(MMError::InitError);
    }

    for region in fdt::get_reserved() {
        let end = region
            .base
            .checked_add(region.size)
            .ok_or(MMError::InitError)?;
        usable.subtract(region.base as usize, end as usize)?;
    }

    let dtb_start = unsafe { crate::dtb_ptr };
    let dtb_end = dtb_start
        .checked_add(fdt::fdt_total_size() as usize)
        .ok_or(MMError::InitError)?;
    usable.subtract(dtb_start, dtb_end)?;

    let (kernel_start, kernel_end) = kernel_image_range();
    usable.subtract(kernel_start, kernel_end)?;

    let min_start = usable
        .get_ranges()
        .iter()
        .map(|r| r.start)
        .min()
        .ok_or(MMError::InitError)?;
    let base = min_start / PAGE_SIZE * PAGE_SIZE;
    let max_end = usable
        .get_ranges()
        .iter()
        .map(|r| r.end)
        .max()
        .ok_or(MMError::InitError)?;
    let num_frames = max_end.saturating_sub(base).div_ceil(PAGE_SIZE);
    let bitmap_words = num_frames.div_ceil(BITS_PER_WORD);
    let bitmap_bytes = bitmap_words
        .checked_mul(size_of::<u64>())
        .ok_or(MMError::InitError)?
        .next_multiple_of(PAGE_SIZE);

    let bitmap_pa = usable
        .get_ranges()
        .iter()
        .find_map(|r| {
            let pa = r.start.next_multiple_of(PAGE_SIZE);
            (pa.checked_add(bitmap_bytes)? <= r.end).then_some(pa)
        })
        .ok_or(MMError::InitError)?;
    usable.subtract(bitmap_pa, bitmap_pa + bitmap_bytes)?;

    let bitmap = get_mut_bitmap();
    bitmap.base = base;
    bitmap.frames = num_frames;
    bitmap.words = unsafe { core::slice::from_raw_parts_mut(bitmap_pa as *mut u64, bitmap_words) };
    bitmap.words.fill(u64::MAX);
    for r in usable.get_ranges() {
        bitmap.set_range(r.start, r.end, false);
    }

    *get_usable() = usable;
    Ok(())
}

pub(crate) fn alloc_frame() -> Result<usize, MMError> {
    let bmap = get_mut_bitmap();
    let pa = bmap
        .alloc_frame_and_return_addr()
        .ok_or(MMError::AllocFrameError)?;
    unsafe { core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE) };
    Ok(pa)
}

pub(crate) fn free_frame(addr: usize) -> Result<(), MMError> {
    let bmap = get_mut_bitmap();
    bmap.free_frame(addr)?;
    Ok(())
}
