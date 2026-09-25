#![allow(dead_code)]

use crate::mm;

const PAGE_TABLE_ENTRIES_PER_PAGE: usize = 512;
const PAGE_SHIFT: usize = 12;
const PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_SHIFT: usize = 10;
const PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_MASK: u64 = (1 << 44) - 1;

const PRESENT: u64 = 1 << 0;
const READ: u64 = 1 << 1;
const WRITE: u64 = 1 << 2;
const EXECUTE: u64 = 1 << 3;
const USER: u64 = 1 << 4;
const GLOBAL: u64 = 1 << 5;
const ACCESSED: u64 = 1 << 6;
const DIRTY: u64 = 1 << 7;

const SUPERVISOR_ADDRESS_TRANSLATION_AND_PROTECTION_MODE_SV39: u64 = 8 << 60;
const SV39_VIRTUAL_ADDRESS_BITS: usize = 39;
const BITS_PER_REGISTER: usize = 64;

struct Frame {
    frames: [u64; 512],
}

pub(crate) enum PagingError {
    InvalidAddress,
    AlreadyMapped,
    OutOfFrames,
}

enum PageTableEntryKind {
    /// V=0.
    NotPresent,
    /// V=1, R=W=X=0: points to the next level page table.
    NextTablePointer,
    /// V=1, R=1 or X=1: final mapping.
    Leaf,
    /// V=1, W=1, R=0: the spec reserves this encoding.
    ReservedWritableWithoutRead,
}

pub(crate) fn map_virtual_to_physical(
    root_table: usize,
    virtual_address: usize,
    physical_address: usize,
    flags: u64,
) -> Result<(), PagingError> {
    if !virtual_address.is_multiple_of(mm::PAGE_SIZE)
        || !physical_address.is_multiple_of(mm::PAGE_SIZE)
    {
        return Err(PagingError::InvalidAddress);
    }
    if !is_canonical(virtual_address) {
        return Err(PagingError::InvalidAddress);
    }
    // TODO: still dunno what the fuck is this?
    if physical_address >> PAGE_SHIFT > PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_MASK as usize {
        return Err(PagingError::InvalidAddress);
    }

    let mut current_table = root_table;
    for level in [2, 1] {
        let index = page_table_index(virtual_address, level);
        let entry = read_page_table_entry(current_table, index);
        match classify_page_table_entry(entry) {
            PageTableEntryKind::NotPresent => {
                let new_table = mm::alloc_frame().map_err(|_| PagingError::OutOfFrames)?;
                write_page_table_entry(
                    current_table,
                    index,
                    physical_address_to_page_table_entry(new_table) | PRESENT,
                );
                current_table = new_table;
            }
            PageTableEntryKind::NextTablePointer => {
                current_table = page_table_entry_to_physical_address(entry);
            }
            PageTableEntryKind::Leaf | PageTableEntryKind::ReservedWritableWithoutRead => {
                return Err(PagingError::AlreadyMapped);
            }
        }
    }

    let index = page_table_index(virtual_address, 0);
    match classify_page_table_entry(read_page_table_entry(current_table, index)) {
        PageTableEntryKind::NotPresent => {
            write_page_table_entry(
                current_table,
                index,
                physical_address_to_page_table_entry(physical_address)
                    | flags
                    | PRESENT
                    | ACCESSED
                    | DIRTY,
            );
            Ok(())
        }
        PageTableEntryKind::NextTablePointer
        | PageTableEntryKind::Leaf
        | PageTableEntryKind::ReservedWritableWithoutRead => Err(PagingError::AlreadyMapped),
    }
}

fn classify_page_table_entry(entry: u64) -> PageTableEntryKind {
    if entry & PRESENT == 0 {
        PageTableEntryKind::NotPresent
    } else if entry & WRITE != 0 && entry & READ == 0 {
        PageTableEntryKind::ReservedWritableWithoutRead
    } else if entry & (READ | WRITE | EXECUTE) == 0 {
        PageTableEntryKind::NextTablePointer
    } else {
        PageTableEntryKind::Leaf
    }
}

fn page_table_index(virtual_address: usize, level: usize) -> usize {
    (virtual_address >> (PAGE_SHIFT + 9 * level)) & (PAGE_TABLE_ENTRIES_PER_PAGE - 1)
}

fn read_page_table_entry(table_physical_address: usize, index: usize) -> u64 {
    unsafe { core::ptr::read((table_physical_address as *const u64).add(index)) }
}

fn write_page_table_entry(table_physical_address: usize, index: usize, entry: u64) {
    unsafe { core::ptr::write((table_physical_address as *mut u64).add(index), entry) };
}

/// Sv39 requires bits 63:39 to equal bit 38.
fn is_canonical(virtual_address: usize) -> bool {
    let shift = BITS_PER_REGISTER - SV39_VIRTUAL_ADDRESS_BITS;
    // `>> shift` on i64 is arithmetic, so it copies bit 38 back into bits 63:39. Equal to `va` only when canonical.
    ((virtual_address as i64) << shift >> shift) == virtual_address as i64
}

fn physical_address_to_page_table_entry(physical_address: usize) -> u64 {
    ((physical_address >> PAGE_SHIFT) as u64) << PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_SHIFT
}

fn page_table_entry_to_physical_address(entry: u64) -> usize {
    (((entry >> PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_SHIFT)
        & PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_MASK)
        << PAGE_SHIFT) as usize
}
