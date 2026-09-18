use core::cell::SyncUnsafeCell;
use core::ptr::{read_volatile, write_volatile};

use crate::utils;
use crate::{fdt, uart};

const ENABLE_OFFSET: usize = 0x0000_2000;
const ENABLE_BYTES_PER_CTX: usize = 0x80;

const THRESHOLD_OFFSET: usize = 0x0020_0000;
const CLAIM_OFFSET: usize = 0x0020_0004;
const PAGE_SIZE_PER_CTX: usize = 0x1000;

const PRIORITY_OFFSET: usize = 0;
const PLIC_REGISTER_SIZE_BYTES: usize = 4;

// Specifier cell value for the S-mode external interrupt of a RISC-V
// CPU-local interrupt controller.
const S_MODE_EXTERNAL_IRQ: u32 = 9;

static PLIC_BASE: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);
static PLIC_CTX: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);
static PLIC_PRESENT: SyncUnsafeCell<bool> = SyncUnsafeCell::new(false);

pub(crate) enum PLICError {
    NoResources,
    BadResources,
    NoContext,
}

pub(crate) fn init(node_idx: usize) -> Result<(), PLICError> {
    let resource = fdt::get_resource(node_idx, 0).ok_or(PLICError::NoResources)?;
    if resource.size == 0 {
        return Err(PLICError::BadResources);
    }
    let context = s_mode_context(node_idx).ok_or(PLICError::NoContext)?;
    let base = usize::try_from(resource.base).map_err(|_| PLICError::BadResources)?;
    unsafe {
        *PLIC_BASE.get() = base;
        *PLIC_CTX.get() = context;
        *PLIC_PRESENT.get() = true;
    }
    if uart::found() {
        program(uart::irq());
    }
    Ok(())
}

pub(crate) fn claim() -> u32 {
    if !present() {
        return 0;
    }
    unsafe { read_volatile(reg_addr(CLAIM_OFFSET + PAGE_SIZE_PER_CTX * ctx()) as *const u32) }
}

pub(crate) fn complete(irq: u32) {
    if !present() || irq == 0 {
        return;
    }
    unsafe {
        write_volatile(
            reg_addr(CLAIM_OFFSET + PAGE_SIZE_PER_CTX * ctx()) as *mut u32,
            irq,
        );
    }
}

fn program(irq: u32) {
    let irq = irq as usize;
    unsafe {
        write_volatile(
            reg_addr(PRIORITY_OFFSET + PLIC_REGISTER_SIZE_BYTES * irq) as *mut u32,
            1,
        );
        write_volatile(
            reg_addr(ENABLE_OFFSET + ENABLE_BYTES_PER_CTX * ctx() + (irq / 32) * 4) as *mut u32,
            1 << (irq % 32),
        );
        write_volatile(
            reg_addr(THRESHOLD_OFFSET + PAGE_SIZE_PER_CTX * ctx()) as *mut u32,
            0,
        );
    }
}

/// Returns the PLIC context id of the current hart's S-mode.
fn s_mode_context(plic_idx: usize) -> Option<usize> {
    let hart_id = utils::current_hart_id()?;
    for i in 0.. {
        let (phandle, irq) = fdt::interrupts_extended(plic_idx, i)?;
        if irq != S_MODE_EXTERNAL_IRQ {
            continue;
        }
        let interrupt_controller = fdt::find_node_by_phandle_prop(phandle)?;
        if hart_id_of(interrupt_controller) == Some(hart_id) {
            return Some(i);
        }
    }
    None
}

/// Returns the hart id of the CPU that owns an interrupt controller.
fn hart_id_of(node_idx: usize) -> Option<u32> {
    let mut cur = fdt::get_parent_by_node_idx(node_idx);
    while let Some(p) = cur {
        if fdt::compatible_has(p, b"riscv") {
            return fdt::find_node_u32_sized_prop_by_name(p, b"reg");
        }
        cur = fdt::get_parent_by_node_idx(p);
    }
    None
}

fn base() -> usize {
    unsafe { *PLIC_BASE.get() }
}

fn ctx() -> usize {
    unsafe { *PLIC_CTX.get() }
}

fn present() -> bool {
    unsafe { *PLIC_PRESENT.get() }
}

fn reg_addr(offset: usize) -> usize {
    base() + offset
}
