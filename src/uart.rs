// qemu specific address
const UART0: u32 = 0x1000_0000;

// the UART control registers.
// see http://byterunner.com/16550.html
const TRANSMITTER_HOLDING_REGISTER_ADDR: u8 = 0; // DLAB = 0
const RECEIVER_BUFFER_REGISTER_ADDR: u8 = TRANSMITTER_HOLDING_REGISTER_ADDR; // DLAB = 0
const LINE_CONTROL_REGISTER_ADDR: u8 = 3;
const DIVISOR_LATCH_LS_ADDR: u8 = 0; // DLAB = 1
const DIVISOR_LATCH_MS_ADDR: u8 = 1; // DLAB = 1

pub(crate) fn init() {
    set_baude_rate();
}

fn set_baude_rate() {
    set_DLAB();
    write_reg(DIVISOR_LATCH_LS_ADDR, 3);
    write_reg(DIVISOR_LATCH_MS_ADDR, 0);
}

fn set_DLAB() {
    write_reg(LINE_CONTROL_REGISTER_ADDR, 1 << 7);
}

fn write_reg(reg_addr: u8, val: u8) {
    unsafe {
        core::ptr::write_volatile((UART0 + reg_addr as u32) as *mut u8, val);
    }
}
