use core::ptr::{read_volatile, write_volatile};

use crate::uart;

const PLIC0: u32 = 0x0C00_0000;

const ENABLE_OFFSET: u32 = 0x0000_2000;
const ENABLE_BYTES_PER_CTX: u32 = 0x80;

const THRESHOLD_OFFSET: u32 = 0x0020_0000;
const CLAIM_OFFSET: u32 = 0x0020_0004;
const PAGE_SIZE_PER_CTX: u32 = 0x1000;

const PRIORITY_OFFSET: u32 = 0;
const PLIC_REGISTER_SIZE_BYTES: u32 = 4;

pub(crate) fn init() {
    unsafe {
        // set priority
        write_volatile(
            (get_abs_addr(PRIORITY_OFFSET) + PLIC_REGISTER_SIZE_BYTES * uart::IRQ) as *mut u32,
            1,
        );
        // set enable bit
        write_volatile(
            (get_abs_addr(ENABLE_OFFSET) + ENABLE_BYTES_PER_CTX * m_mode_ctx(0)) as *mut u32,
            1 << uart::IRQ,
        );
        // set threshold bit
        write_volatile(
            (get_abs_addr(THRESHOLD_OFFSET) + PAGE_SIZE_PER_CTX * m_mode_ctx(0)) as *mut u32,
            0,
        );
    }
}

pub(crate) fn claim() -> u32 {
    unsafe {
        read_volatile(
            (get_abs_addr(CLAIM_OFFSET) + PAGE_SIZE_PER_CTX * m_mode_ctx(0)) as *const u32,
        )
    }
}

pub(crate) fn complete(irq: u32) {
    unsafe {
        write_volatile(
            (get_abs_addr(CLAIM_OFFSET) + PAGE_SIZE_PER_CTX * m_mode_ctx(0)) as *mut u32,
            irq,
        );
    }
}

fn get_abs_addr(offset: u32) -> u32 {
    PLIC0 + offset
}

fn m_mode_ctx(hart_id: u32) -> u32 {
    2 * hart_id
}
