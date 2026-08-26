#![no_std]
#![no_main]
#![feature(sync_unsafe_cell)]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod kernel_trap;
mod plic;
mod uart;

global_asm!(
    r#"
    .section .bss
    .align 4
stack0:
    .space 4096 * 8          // 4KB stack per hart, up to 8 harts

    .section .text.entry
    .global _entry
_entry:
    la   sp, stack0
    csrr a0, mhartid
    addi a0, a0, 1
    slli a0, a0, 12
    add  sp, sp, a0          // sp = stack0 + (hartid+1)*4KB
    call start
"#
);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

fn main() {
    loop {
        match uart::pop_byte() {
            Some(byte) => uart::send_byte(byte),
            None => unsafe {
                asm!("wfi");
            },
        }
    }
}

fn timer_init() {}

fn handover_to_s_mode() {
    unsafe {
        // trap routine
        asm!("csrw medeleg, {0}", in(reg) 0xffffusize);
        asm!("csrw mideleg, {0}", in(reg) 0xffffusize);

        // memory access for s mode
        asm!("csrw pmpaddr0, {0}", in(reg) 0x3f_ffff_ffff_ffffusize);
        asm!("csrw pmpcfg0, {0}", in(reg) 0xfusize);

        // paging
        asm!("csrw satp, 0");

        // handoff target
        asm!("csrw mepc, {0}", in(reg) main as *const () as usize);

        // drop to s mode after mret
        asm!("csrc mstatus, {0}", in(reg) 3usize << 11);
        asm!("csrs mstatus, {0}", in(reg) 1usize << 11);

        // context handover
        asm!("csrr tp, mhartid");

        // timer
        timer_init();

        // perform the drop, hint to rust compiler that main never returns
        asm!("mret", options(noreturn));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn start() {
    uart::init();
    plic::init();

    handover_to_s_mode();
}
