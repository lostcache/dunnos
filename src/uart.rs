const UART0: u32 = 0x10000000;

// the UART control registers.
// some have different meanings for read vs write.
// see http://byterunner.com/16550.html
const RHR: u32 = 0; // receive holding register (for input bytes)
const THR: u32 = 0; // transmit holding register (for output bytes)
const IER: u32 = 1; // interrupt enable register
const IER_RX_ENABLE: u32 = 1 << 0; // receiver interrupts
const IER_TX_ENABLE: u32 = 1 << 1; // transmit interrupts
const FCR: u32 = 2; // FIFO control register
const FCR_FIFO_ENABLE: u32 = 1 << 0;
const FCR_FIFO_CLEAR: u32 = 3 << 1; // clear the content of the two FIFOs
const ISR: u32 = 2; // interrupt status register
const LCR: u32 = 3; // line control register
const LCR_EIGHT_BITS: u32 = 3 << 0;
const LCR_BAUD_LATCH: u32 = 1 << 7; // special mode to set baud rate
const LSR: u32 = 5; // line status register
const LSR_RX_READY: u32 = 1 << 0; // input is waiting to be read from RHR
const LSR_TX_IDLE: u32 = 1 << 5; // THR can accept another character to send
