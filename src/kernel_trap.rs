use core::arch::{asm, global_asm};
use core::mem::offset_of;

use crate::{plic, timer_interrupt, uart};

const SUPERVISOR_EXTERNAL_INTERRUPT_CODE: usize = 9;
const TIMER_INTERRUPT: usize = 5;
const INSTRUCTION_PAGE_FAULT: usize = 12;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;

#[repr(C)]
pub(crate) struct TrapFrame {
    pub(crate) x1: usize,
    pub(crate) x2: usize,
    pub(crate) x3: usize,
    pub(crate) x4: usize,
    pub(crate) x5: usize,
    pub(crate) x6: usize,
    pub(crate) x7: usize,
    pub(crate) x8: usize,
    pub(crate) x9: usize,
    pub(crate) x10: usize,
    pub(crate) x11: usize,
    pub(crate) x12: usize,
    pub(crate) x13: usize,
    pub(crate) x14: usize,
    pub(crate) x15: usize,
    pub(crate) x16: usize,
    pub(crate) x17: usize,
    pub(crate) x18: usize,
    pub(crate) x19: usize,
    pub(crate) x20: usize,
    pub(crate) x21: usize,
    pub(crate) x22: usize,
    pub(crate) x23: usize,
    pub(crate) x24: usize,
    pub(crate) x25: usize,
    pub(crate) x26: usize,
    pub(crate) x27: usize,
    pub(crate) x28: usize,
    pub(crate) x29: usize,
    pub(crate) x30: usize,
    pub(crate) x31: usize,
    pub(crate) sstatus: usize,
    pub(crate) sepc: usize,
    pub(crate) scause: usize,
    pub(crate) stval: usize,
    pub(crate) pad: usize,
}

// All registers except x0 and x2 are saved. x0 is constant zero. x2
// (sp) needs no save: the prologue subtracts a constant that the
// epilogue adds back.
global_asm!(
    r#"
    .global _kerneltrapvec
    .align 2
_kerneltrapvec:
    addi sp, sp, -{frame}

    sd x1, {x1}(sp)
    sd x3, {x3}(sp)
    sd x4, {x4}(sp)
    sd x5, {x5}(sp)
    sd x6, {x6}(sp)
    sd x7, {x7}(sp)
    sd x8, {x8}(sp)
    sd x9, {x9}(sp)
    sd x10, {x10}(sp)
    sd x11, {x11}(sp)
    sd x12, {x12}(sp)
    sd x13, {x13}(sp)
    sd x14, {x14}(sp)
    sd x15, {x15}(sp)
    sd x16, {x16}(sp)
    sd x17, {x17}(sp)
    sd x18, {x18}(sp)
    sd x19, {x19}(sp)
    sd x20, {x20}(sp)
    sd x21, {x21}(sp)
    sd x22, {x22}(sp)
    sd x23, {x23}(sp)
    sd x24, {x24}(sp)
    sd x25, {x25}(sp)
    sd x26, {x26}(sp)
    sd x27, {x27}(sp)
    sd x28, {x28}(sp)
    sd x29, {x29}(sp)
    sd x30, {x30}(sp)
    sd x31, {x31}(sp)
    csrr t1, sstatus
    sd t1, {sstatus}(sp)
    csrr t1, sepc
    sd t1, {sepc}(sp)
    csrr t1, scause
    sd t1, {scause}(sp)
    csrr t1, stval
    sd t1, {stval}(sp)

    call kerneltrap

    ld t0, {sstatus}(sp)
    csrw sstratus, t0
    ld t0, {sepc}(sp)
    csrw sepc, t0
    ld x1, {x1}(sp)
    ld x3, {x3}(sp)
    ld x4, {x4}(sp)
    ld x5, {x5}(sp)
    ld x6, {x6}(sp)
    ld x7, {x7}(sp)
    ld x8, {x8}(sp)
    ld x9, {x9}(sp)
    ld x10, {x10}(sp)
    ld x11, {x11}(sp)
    ld x12, {x12}(sp)
    ld x13, {x13}(sp)
    ld x14, {x14}(sp)
    ld x15, {x15}(sp)
    ld x16, {x16}(sp)
    ld x17, {x17}(sp)
    ld x18, {x18}(sp)
    ld x19, {x19}(sp)
    ld x20, {x20}(sp)
    ld x21, {x21}(sp)
    ld x22, {x22}(sp)
    ld x23, {x23}(sp)
    ld x24, {x24}(sp)
    ld x25, {x25}(sp)
    ld x26, {x26}(sp)
    ld x27, {x27}(sp)
    ld x28, {x28}(sp)
    ld x29, {x29}(sp)
    ld x30, {x30}(sp)
    ld x31, {x31}(sp)

    ld sp, {x2}(sp)

    sret
    "#,
    frame = const size_of::<TrapFrame>(),
    x1 = const offset_of!(TrapFrame, x1),
    x2 = const offset_of!(TrapFrame, x1),
    x3 = const offset_of!(TrapFrame, x1),
    x4 = const offset_of!(TrapFrame, x1),
    x5 = const offset_of!(TrapFrame, x1),
    x6 = const offset_of!(TrapFrame, x1),
    x7 = const offset_of!(TrapFrame, x1),
    x8 = const offset_of!(TrapFrame, x1),
    x9 = const offset_of!(TrapFrame, x1),
    x10 = const offset_of!(TrapFrame, x1),
    x11 = const offset_of!(TrapFrame, x1),
    x12 = const offset_of!(TrapFrame, x1),
    x13 = const offset_of!(TrapFrame, x1),
    x14 = const offset_of!(TrapFrame, x1),
    x15 = const offset_of!(TrapFrame, x1),
    x16 = const offset_of!(TrapFrame, x1),
    x17 = const offset_of!(TrapFrame, x1),
    x18 = const offset_of!(TrapFrame, x1),
    x19 = const offset_of!(TrapFrame, x1),
    x20 = const offset_of!(TrapFrame, x1),
    x21 = const offset_of!(TrapFrame, x1),
    x22 = const offset_of!(TrapFrame, x1),
    x23 = const offset_of!(TrapFrame, x1),
    x24 = const offset_of!(TrapFrame, x1),
    x25 = const offset_of!(TrapFrame, x1),
    x26 = const offset_of!(TrapFrame, x1),
    x27 = const offset_of!(TrapFrame, x1),
    x28 = const offset_of!(TrapFrame, x1),
    x29 = const offset_of!(TrapFrame, x1),
    x30 = const offset_of!(TrapFrame, x1),
    x31 = const offset_of!(TrapFrame, x1),
    sstatus = const offset_of!(TrapFrame, x1),
    sepc = const offset_of!(TrapFrame, x1),
    scause = const offset_of!(TrapFrame, x1),
    stval = const offset_of!(TrapFrame, x1),
);

#[unsafe(no_mangle)]
extern "C" fn kerneltrap() {
    let cause: usize;
    unsafe {
        asm!("csrr {0}, scause", out(reg) cause);
    }
    let is_interrupt = cause >> 63 != 0;
    let code = cause & 0xff;
    if is_interrupt {
        match code {
            SUPERVISOR_EXTERNAL_INTERRUPT_CODE => {
                let irq = plic::claim();
                if irq == uart::irq() {
                    uart::handle_interrupt();
                }
                if irq != 0 {
                    plic::complete(irq);
                }
            }
            TIMER_INTERRUPT => timer_interrupt::handle_timer_interrupt(),
            _ => loop {
                core::hint::spin_loop();
            },
        }
    } else {
        match code {
            INSTRUCTION_PAGE_FAULT | LOAD_PAGE_FAULT | STORE_PAGE_FAULT => report_page_fault(cause),
            _ => loop {
                core::hint::spin_loop();
            },
        }
    }
}

fn report_page_fault(scause: usize) -> ! {
    let stval: usize;
    let sepc: usize;
    unsafe {
        asm!("csrr {0}, stval", out(reg) stval);
        asm!("csrr {0}, sepc", out(reg) sepc);
    }
    uart::send_str(b"\nFAULT scause=");
    uart::send_hex(scause);
    uart::send_str(b" stval=");
    uart::send_hex(stval);
    uart::send_str(b" sepc=");
    uart::send_hex(sepc);
    uart::send_str(b"\n");
    loop {
        unsafe { asm!("wfi") }
    }
}

unsafe extern "C" {
    pub(crate) fn _kerneltrapvec();
}
