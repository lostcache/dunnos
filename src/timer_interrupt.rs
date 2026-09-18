use crate::uart;
use core::arch::asm;
use core::cell::SyncUnsafeCell;

static TICKS: SyncUnsafeCell<u64> = SyncUnsafeCell::new(0);

const TIMER_INTERVAL: u64 = 1_000_000;

fn tick() {
    let ticks = unsafe { &mut *TICKS.get() };
    *ticks += 1;
    if *ticks % 10 == 0 {
        uart::send_byte(b'.');
    }
}

/// Arms the next timer interrupt.
pub(crate) fn set_timer_interval_for_interrupt() {
    let now: u64;
    unsafe {
        asm!("csrr {0}, time", out(reg) now);
        asm!("csrw stimecmp, {0}", in(reg) now + TIMER_INTERVAL);
    }
}

/// Enables the S-mode timer interrupt. Runs in M-mode.
pub(crate) fn timer_init() {
    unsafe {
        asm!("csrs menvcfg, {0}", in(reg) 1usize << 63); // STCE: allow S-mode access to stimecmp.
        asm!("csrs mcounteren, {0}", in(reg) 1usize << 1); // TM: allow S-mode reads of the time CSR.
    }
    set_timer_interval_for_interrupt();
    unsafe {
        asm!("csrs mie, {0}", in(reg) 1usize << 5); // STIE: enable the supervisor timer interrupt.
    }
}

pub(crate) fn handle_timer_interrupt() {
    set_timer_interval_for_interrupt();
    tick();
}
