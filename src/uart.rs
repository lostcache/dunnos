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

fn send(val: u8) {
    set_line_control_register_for_transmission();
    write_reg(TRANSMITTER_HOLDING_REGISTER_ADDR, val);
}

fn set_line_control_register_for_transmission() {
    const CHAR_LEN: u8 = (1 << 0) | (1 << 1); // 8 bit char
    const NUM_STOP_BITS: u8 = 0 << 2; // 0->1 and 1->2 stop bits, receiver only uses first.
    const PARITY_BIT: u8 = 1 << 3;
    const EVEN_PARITY_SELECT: u8 = 0 << 4;
    const STICK_PARITY: u8 = 0 << 5; // not needed
    const BREAK_CONTROL_BIT: u8 = 0 << 6;
    const DIVISOR_LATCH_ACCESS: u8 = 0 << 7;
    write_reg(
        LINE_CONTROL_REGISTER_ADDR,
        CHAR_LEN
            | NUM_STOP_BITS
            | PARITY_BIT
            | EVEN_PARITY_SELECT
            | STICK_PARITY
            | BREAK_CONTROL_BIT
            | DIVISOR_LATCH_ACCESS,
    );
}

fn set_DLAB() {
    write_reg(LINE_CONTROL_REGISTER_ADDR, 1 << 7);
}

fn write_reg(reg_addr: u8, val: u8) {
    unsafe {
        core::ptr::write_volatile((UART0 + reg_addr as u32) as *mut u8, val);
    }
}
