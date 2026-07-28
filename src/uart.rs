use core::cell::SyncUnsafeCell;

// qemu specific address
pub(crate) const IRQ: u32 = 10;
const UART0: u32 = 0x1000_0000;

// the UART control registers.
// see http://byterunner.com/16550.html
const TRANSMITTER_HOLDING_REGISTER_ADDR: u8 = 0; // DLAB = 0
const RECEIVER_BUFFER_REGISTER_ADDR: u8 = TRANSMITTER_HOLDING_REGISTER_ADDR; // DLAB = 0
const LINE_CONTROL_REGISTER_ADDR: u8 = 3;
const DIVISOR_LATCH_LS_ADDR: u8 = 0; // DLAB = 1
const DIVISOR_LATCH_MS_ADDR: u8 = 1; // DLAB = 1
const INTERRUPT_IDENTIFIER_REGISTER_ADDR: u8 = 2;
const IO_BUFFER_SIZE: usize = 128;
const LINE_STATUS_REGISTER_ADDR: u8 = 5;

pub(crate) fn init() {
    set_baude_rate();
    init_interrtup_enable_register();
    set_line_control_register_for_transmission_and_reception();
}

pub(crate) fn handle_interrupt() {
    const RECEIVER_LINE_STATUS_INTERRUPT: u8 = (1 << 2) | (1 << 1);
    const RECEIVER_DATA_AVAILABLE_INTERRUPT: u8 = 1 << 2;
    const RECEIVER_TRANSMITTER_HOLDING_EMPTY_INTERRUPT: u8 = 1 << 1;
    let interrupt_identity_register_val = read_reg(INTERRUPT_IDENTIFIER_REGISTER_ADDR);
    match interrupt_identity_register_val & 0x0F {
        RECEIVER_LINE_STATUS_INTERRUPT => {
            // overun, parity or framing error.
            read_reg(LINE_STATUS_REGISTER_ADDR);
        }
        RECEIVER_DATA_AVAILABLE_INTERRUPT => {
            match push_byte(read_byte()) {
                Err(IOError::BufferOverflow) | Ok(()) => {
                    // drops the byte if buffer overflows
                }
            }
        }
        RECEIVER_TRANSMITTER_HOLDING_EMPTY_INTERRUPT => {}
        _ => (),
    }
}

pub(crate) fn pop_byte() -> Option<u8> {
    let buf = unsafe { &mut *IO_BUFFER.get() };
    if buf.read == buf.write {
        return None;
    }
    let byte = buf.data[buf.read];
    buf.read = (buf.read + 1) % IO_BUFFER_SIZE;
    Some(byte)
}

pub(crate) fn send_byte(val: u8) {
    write_reg(TRANSMITTER_HOLDING_REGISTER_ADDR, val);
}

enum IOError {
    BufferOverflow,
}

struct RingBuffer {
    read: usize,
    write: usize,
    data: [u8; IO_BUFFER_SIZE],
}

static IO_BUFFER: SyncUnsafeCell<RingBuffer> = SyncUnsafeCell::new(RingBuffer {
    read: 0,
    write: 0,
    data: [0; IO_BUFFER_SIZE],
});

fn set_baude_rate() {
    set_dlab_bit();
    write_reg(DIVISOR_LATCH_LS_ADDR, 3);
    write_reg(DIVISOR_LATCH_MS_ADDR, 0);
}

fn push_byte(val: u8) -> Result<(), IOError> {
    let buf = unsafe { &mut *IO_BUFFER.get() };
    if (buf.write + 1) % IO_BUFFER_SIZE == buf.read {
        return Err(IOError::BufferOverflow);
    }
    buf.data[buf.write] = val;
    buf.write = (buf.write + 1) % IO_BUFFER_SIZE;
    Ok(())
}

fn read_byte() -> u8 {
    read_reg(RECEIVER_BUFFER_REGISTER_ADDR)
}

fn set_line_control_register_for_transmission_and_reception() {
    const CHAR_LEN: u8 = (1 << 0) | (1 << 1); // 8 bit char
    const NUM_STOP_BITS: u8 = 0 << 2; // 0->1 and 1->2 stop bits, receiver only uses first.
    const PARITY_BIT: u8 = 0 << 3;
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

fn set_dlab_bit() {
    write_reg(LINE_CONTROL_REGISTER_ADDR, 1 << 7);
}

fn read_reg(reg_addr: u8) -> u8 {
    unsafe { core::ptr::read_volatile(get_mem_addr(reg_addr) as *mut u8) }
}

fn write_reg(reg_addr: u8, val: u8) {
    unsafe {
        core::ptr::write_volatile(get_mem_addr(reg_addr) as *mut u8, val);
    }
}

fn get_mem_addr(offset: u8) -> u32 {
    UART0 + u32::from(offset)
}

fn init_interrtup_enable_register() {
    const INTERRUPT_ENABLE_REGISTER_ADDR: u8 = 1;
    const RECEIVED_DATA_AVAILABLE_INTERRUPT: u8 = 1 << 0;
    const TRANSMITTER_HOLDING_REGISTER_EMPTY_INTERRUPT: u8 = 0 << 1; // The holding register is empty almost always. The interrupt would fire without end.
    const REVEIVER_LINE_STAATUS_INTERRUPT: u8 = 1 << 2;
    write_reg(
        INTERRUPT_ENABLE_REGISTER_ADDR,
        RECEIVED_DATA_AVAILABLE_INTERRUPT
            | TRANSMITTER_HOLDING_REGISTER_EMPTY_INTERRUPT
            | REVEIVER_LINE_STAATUS_INTERRUPT,
    );
}
