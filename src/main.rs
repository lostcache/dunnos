#![no_std]
#![no_main]
#![feature(sync_unsafe_cell)]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod fdt;
mod kernel_trap;
mod plic;
mod timer_interrupt;
mod uart;
mod utils;

// The entry asm stores the DTB address from a1 here. Only asm
// references this symbol, so #[used] stops the compiler from
// discarding it.
#[used]
#[unsafe(no_mangle)]
pub(crate) static mut dtb_ptr: usize = 0;

global_asm!(
    r#"
    .section .bss
    .align 4
stack0:
    .space 4096 * 8          // One 4KB stack per hart, up to 8 harts.

    .section .text.entry
    .global _entry
_entry:
    la   sp, stack0
    csrr a0, mhartid
    addi a0, a0, 1
    slli a0, a0, 12
    add  sp, sp, a0          // sp = top of this hart's stack slot.

    la   t0, dtb_ptr
    sd   a1, 0(t0)

    call start
"#
);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

fn main() {
    unsafe {
        asm!("csrs sstatus, {0}", in(reg) 1usize << 1); // SIE: enable interrupts in S-mode.
    }
    loop {
        match uart::pop_byte() {
            Some(byte) => uart::send_byte(byte),
            None => unsafe {
                asm!("wfi");
            },
        }
    }
}

fn handover_to_s_mode() {
    unsafe {
        // Delegate traps to S-mode.
        asm!("csrw medeleg, {0}", in(reg) 0xffffusize);
        asm!("csrw mideleg, {0}", in(reg) 0xffffusize);

        // S-mode trap vector and external interrupt enable.
        asm!("csrw stvec, {0}", in(reg) kernel_trap::_kerneltrapvec as *const () as usize);
        asm!("csrs sie, {0}", in(reg) 1usize << 9); // SEIE: enable supervisor external interrupts.

        // PMP: allow S-mode access to all physical memory.
        asm!("csrw pmpaddr0, {0}", in(reg) 0x3f_ffff_ffff_ffffusize);
        asm!("csrw pmpcfg0, {0}", in(reg) 0xfusize);

        // satp = 0: disable paging.
        asm!("csrw satp, 0");

        // mret enters the S-mode entry point.
        asm!("csrw mepc, {0}", in(reg) main as *const () as usize);

        // MPP = S: mret drops to S-mode.
        asm!("csrc mstatus, {0}", in(reg) 3usize << 11);
        asm!("csrs mstatus, {0}", in(reg) 1usize << 11);

        // tp = hart id.
        asm!("csrr tp, mhartid");

        // Arm the S-mode timer.
        timer_interrupt::timer_init();

        // mret never returns.
        asm!("mret", options(noreturn));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn start() {
    let fdt_result = fdt::probe();

    if fdt_result.is_ok() {
        let uart_ok =
            fdt::find_compatible_node_idx(b"ns16550a").is_some_and(|node| uart::init(node).is_ok());
        let plic_ok = fdt::find_compatible_node_idx(b"riscv,plic0")
            .is_some_and(|node| plic::init(node).is_ok());

        if uart::found() {
            uart::send_byte(b'F');
            uart::send_byte(if uart_ok && plic_ok { b'+' } else { b'-' });
        }
    }

    handover_to_s_mode();
}
