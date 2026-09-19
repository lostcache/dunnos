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

// Written by _entry asm (bootloader's a1 = DTB address). #[used]: asm-only reference must survive GC.
#[used]
#[unsafe(no_mangle)]
pub(crate) static mut dtb_ptr: usize = 0;

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
        asm!("csrs sstatus, {0}", in(reg) 1usize << 1); // SIE: take interrupts in S-mode
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
        // trap routine
        asm!("csrw medeleg, {0}", in(reg) 0xffffusize);
        asm!("csrw mideleg, {0}", in(reg) 0xffffusize);

        // S-mode trap vector and external interrupt enables
        asm!("csrw stvec, {0}", in(reg) kernel_trap::_kerneltrapvec as *const () as usize);
        asm!("csrs sie, {0}", in(reg) 1usize << 9); // SEIE: supervisor external interrupts

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
        timer_interrupt::timer_init();

        // perform the drop, hint to rust compiler that main never returns
        asm!("mret", options(noreturn));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn start() {
    let fdt_result = fdt::probe();

    match fdt_result {
        Ok(()) => {
            let uart_ok = fdt::find_compatible_node_idx(b"ns16550a")
                .is_some_and(|node| uart::init(node).is_ok());
            if uart::found() {
                uart::send_byte(b'F');
                uart::send_byte(if uart_ok { b'+' } else { b'-' });
            }
        }
        Err(e) => uart::send_byte(match e {
            fdt::FdtError::NoDtb => b'D',
            fdt::FdtError::BadHeader => b'H',
            fdt::FdtError::BadStructure => b'S',
            fdt::FdtError::ArenaFull => b'A',
        }),
    }

    plic::init();

    handover_to_s_mode();
}
