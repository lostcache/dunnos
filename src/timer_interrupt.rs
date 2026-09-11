use crate::uart;
use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::ptr::{read_volatile, write_volatile};

static TICKS: SyncUnsafeCell<u64> = SyncUnsafeCell::new(0);

const CLINT_MTIME: usize = 0x0200_BFF8;
const CLINT_MTIMECMP: usize = 0x0200_4000;
const TIMER_INTERVAL: u64 = 1_000_000;

fn tick() {
    let ticks = unsafe { &mut *TICKS.get() };
    *ticks += 1;
    if *ticks % 10 == 0 {
        uart::send_byte(b'.');
    }
}

pub(crate) fn set_timer_interval_for_interrupt() {
    let hart: usize;
    unsafe {
        asm!("mv {0}, tp", out(reg) hart);
        let now = read_volatile(CLINT_MTIME as *const u64);
        write_volatile(
            (CLINT_MTIMECMP + 8 * hart) as *mut u64,
            now + TIMER_INTERVAL,
        );
    }
}

pub(crate) fn timer_init() {
    set_timer_interval_for_interrupt();
    unsafe {
        asm!("csrs mie, {0}", in(reg) 1usize << 5); // STIE
    }
}

pub(crate) fn handle_timer_interrupt() {
    set_timer_interval_for_interrupt();
    tick();
}
