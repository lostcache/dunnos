use core::arch::{asm, global_asm};

use crate::{plic, uart};

const MACHINE_EXTERNAL_INTERRUPT_CODE: usize = 11;

pub(crate) unsafe fn kernel_trap_init() {
    unsafe {
        asm!("csrw mtvec, {0}", in(reg) _kerneltrapvec as *const () as usize);
        asm!("li {0}, 1 << 11", "csrs mie, {0}", out(reg) _);
        asm!("csrs mstatus, {0}", in(reg) 1usize << 3);
    }
}

#[unsafe(no_mangle)]
extern "C" fn kerneltrap() {
    let cause: usize;
    unsafe {
        asm!("csrr {0}, mcause", out(reg) cause);
    }
    let is_interrupt = cause >> 63 != 0;
    let code = cause & 0xff;
    if is_interrupt && code == MACHINE_EXTERNAL_INTERRUPT_CODE {
        let irq = plic::claim();
        if irq == uart::IRQ {
            uart::handle_interrupt();
        }
        if irq != 0 {
            plic::complete(irq);
        }
    } else {
        loop {}
    }
}

unsafe extern "C" {
    pub(crate) fn _kerneltrapvec();
}

// x0 is hardwired to zero, x2 (sp) is restored arithmetically.
global_asm!(
    r#"
    .global _kerneltrapvec
    .align 2
_kerneltrapvec:
    addi sp, sp, -256
    sd x1, 0(sp)
    sd x3, 16(sp)
    sd x4, 24(sp)
    sd x5, 32(sp)
    sd x6, 40(sp)
    sd x7, 48(sp)
    sd x8, 56(sp)
    sd x9, 64(sp)
    sd x10, 72(sp)
    sd x11, 80(sp)
    sd x12, 88(sp)
    sd x13, 96(sp)
    sd x14, 104(sp)
    sd x15, 112(sp)
    sd x16, 120(sp)
    sd x17, 128(sp)
    sd x18, 136(sp)
    sd x19, 144(sp)
    sd x20, 152(sp)
    sd x21, 160(sp)
    sd x22, 168(sp)
    sd x23, 176(sp)
    sd x24, 184(sp)
    sd x25, 192(sp)
    sd x26, 200(sp)
    sd x27, 208(sp)
    sd x28, 216(sp)
    sd x29, 224(sp)
    sd x30, 232(sp)
    sd x31, 240(sp)

    call kerneltrap

    ld x1, 0(sp)
    ld x3, 16(sp)
    ld x4, 24(sp)
    ld x5, 32(sp)
    ld x6, 40(sp)
    ld x7, 48(sp)
    ld x8, 56(sp)
    ld x9, 64(sp)
    ld x10, 72(sp)
    ld x11, 80(sp)
    ld x12, 88(sp)
    ld x13, 96(sp)
    ld x14, 104(sp)
    ld x15, 112(sp)
    ld x16, 120(sp)
    ld x17, 128(sp)
    ld x18, 136(sp)
    ld x19, 144(sp)
    ld x20, 152(sp)
    ld x21, 160(sp)
    ld x22, 168(sp)
    ld x23, 176(sp)
    ld x24, 184(sp)
    ld x25, 192(sp)
    ld x26, 200(sp)
    ld x27, 208(sp)
    ld x28, 216(sp)
    ld x29, 224(sp)
    ld x30, 232(sp)
    ld x31, 240(sp)

    addi sp, sp, 256

    mret
    "#
);
