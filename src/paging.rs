use core::arch::asm;

use crate::{fdt, mm};

const PAGE_TABLE_ENTRIES_PER_PAGE: usize = 512;
const PAGE_SHIFT: usize = 12;
const PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_SHIFT: usize = 10;
const PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_MASK: u64 = (1 << 44) - 1;

const PTE_PRESENT: u64 = 1 << 0;
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

/// PBMT = 01: input/output memory type. PTE bits 62:61.
const PHYSICAL_MEMORY_TYPE_INPUT_OUTPUT: u64 = 1 << 61;

/// Flags for each identity-mapped RAM page. Execute is set because
/// the kernel image lives in RAM. Global is set: one kernel map.
const IDENTITY_MAP_FLAGS: u64 = READ | WRITE | EXECUTE | GLOBAL;

/// Flags for each MMIO page. No execute: device registers are data.
const MMIO_FLAGS: u64 = READ | WRITE | GLOBAL;

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

/// Builds the kernel page tables in machine mode (M-mode).
/// Returns the physical address of the root table.
pub(crate) fn init() -> Result<usize, PagingError> {
    let mut mmio_flags = MMIO_FLAGS;
    if svpbmt_supported() {
        set_page_based_memory_type_enable_bit();
        mmio_flags |= PHYSICAL_MEMORY_TYPE_INPUT_OUTPUT;
    }

    let root = mm::alloc_frame().map_err(|_| PagingError::OutOfFrames)?;

    for range in mm::get_physical_ranges() {
        map_identity_range(root, range.start, range.end, IDENTITY_MAP_FLAGS)?;
    }

    map_mmio_device(root, b"ns16550a", mmio_flags)?;
    map_mmio_device(root, b"riscv,plic0", mmio_flags)?;

    Ok(root)
}

/// Sets menvcfg bit 62 (PBMTE). Machine mode only.
fn set_page_based_memory_type_enable_bit() {
    const PAGE_BASED_MEMORY_TYPE_ENABLE_BIT: usize = 1 << 62;
    unsafe {
        asm!("csrs menvcfg, {0}", in(reg) PAGE_BASED_MEMORY_TYPE_ENABLE_BIT);
    }
}

fn svpbmt_supported() -> bool {
    let Some(cpu_node_idx) = fdt::get_node_idx_by_prop_name_and_val(b"device_type", b"cpu") else {
        return false;
    };
    let Some(isa) = fdt::get_node_prop_value_by_name(cpu_node_idx, b"riscv,isa") else {
        return false;
    };
    let Some(len) = isa.iter().position(|&b| b == 0) else {
        return false;
    };
    isa[..len]
        .split(|&b| b == b'_')
        .any(|token| token == b"svpbmt".as_slice())
}

/// Identity maps the reg resource of the first node that lists the
/// given compatible string. Does nothing when the node or the
/// resource is absent. Extra mappings are harmless.
fn map_mmio_device(root: usize, compatible: &[u8], flags: u64) -> Result<(), PagingError> {
    // TODO: why are we returning Ok if compatible is None?
    let Some(node_idx) = fdt::find_compatible_node_idx(compatible) else {
        return Ok(());
    };

    // TODO: why are we returning Ok if get_resource is None?
    let Some(resource) = fdt::get_resource(node_idx, 0) else {
        return Ok(());
    };

    let Ok(base) = usize::try_from(resource.base) else {
        return Ok(());
    };

    let Some(end) = usize::try_from(resource.size)
        .ok()
        .and_then(|size| base.checked_add(size))
    else {
        return Ok(());
    };

    map_identity_range(root, base, end, flags)
}

/// Identity maps every 4 KiB page that [start, end) touches.
/// Aligns the start down and the end up, so a sub-page device is
/// mapped. Skips a page that is mapped already:
/// two devices may share a page, and RAM ranges may overlap.
fn map_identity_range(
    root: usize,
    start: usize,
    end: usize,
    flags: u64,
) -> Result<(), PagingError> {
    let first_page = start & !(mm::PAGE_SIZE - 1);
    let last_page = end.next_multiple_of(mm::PAGE_SIZE);
    for addr in (first_page..last_page).step_by(mm::PAGE_SIZE) {
        match map_virtual_to_physical(root, addr, addr, flags) {
            Ok(()) | Err(PagingError::AlreadyMapped) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
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

    if (physical_address >> PAGE_SHIFT) as u64 > PAGE_TABLE_ENTRY_PHYSICAL_PAGE_NUMBER_MASK {
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
                    physical_address_to_page_table_entry(new_table) | PTE_PRESENT,
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
                    | PTE_PRESENT
                    | ACCESSED
                    | DIRTY
                    | GLOBAL, // Need to remove this later for ASID != 0
            );
            Ok(())
        }
        PageTableEntryKind::NextTablePointer
        | PageTableEntryKind::Leaf
        | PageTableEntryKind::ReservedWritableWithoutRead => Err(PagingError::AlreadyMapped),
    }
}

fn classify_page_table_entry(entry: u64) -> PageTableEntryKind {
    if entry & PTE_PRESENT == 0 {
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
