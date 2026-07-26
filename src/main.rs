#![no_std]
#![no_main]
#![feature(sync_unsafe_cell)]

use core::arch::global_asm;
use core::panic::PanicInfo;

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

#[unsafe(no_mangle)]
pub extern "C" fn start() -> ! {
    uart::init();
    loop {
        uart::handle_interrupt();
        if let Some(byte) = uart::pop_byte() {
            uart::send_byte(byte);
        }
    }
}
