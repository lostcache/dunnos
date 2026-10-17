# Registers in this kernel

Three register classes exist in this codebase:

1. **Integer registers.** The 32 general registers x0 to x31 of each hart. A hart is one hardware thread of the central processing unit (CPU).
2. **Control and status registers (CSR).** System registers. Machine-mode and supervisor-mode CSRs exist. See the privilege levels below.
3. **Device registers.** Memory-mapped input/output (MMIO): device cells inside the address space, reached through normal loads and stores. The platform-level interrupt controller (PLIC) and the universal asynchronous receiver-transmitter (UART) use this class.

A hart runs one privilege level at a time. Three levels exist. The lowest level has the fewest rights.

| Level | Name | Rights | Code in this kernel |
|---|---|---|---|
| M-mode | Machine mode | Highest. Every CSR is open. | The boot path in `main.rs`: probe of the device tree, page table build, timer setup. |
| S-mode | Supervisor mode | The kernel level. Owns paging, traps and devices. Size of the CSRs above. | `main()`, the trap handlers, the scheduler. |
| U-mode | User mode | Lowest. Reaches no CSR. Reaches only pages that carry the U bit. | The user program of a task. Steps 25 to 28 run code here. |

The calling convention splits the integer set: a caller saves the `t` and `a` registers before a call, a callee saves the `s` registers it uses. This rule makes the context switch cheap: `switch_to` saves only `ra`, `sp` and the `s` registers.

## Integer registers

| Register | Name | Class | Purpose in RISC-V | Use in this kernel |
|---|---|---|---|---|
| x0 | zero | — | Reads as zero. Writes have no effect. | Address zero for null pointers. |
| x1 | ra | Caller | Return address. Call instructions write it. | Trap frame holds it. `switch_to` saves it. |
| x2 | sp | Caller | Stack pointer. Points at the last stack byte in use. Grows down. | Trap frame stores the old stack pointer. |
| x3 | gp | Callee | Global pointer. Anchor for small data. | Unused. |
| x4 | tp | Callee | Thread pointer. Holds thread-local storage data. | Holds the hart ID after handover. |
| x5-x7, x28-x31 | t0-t6 | Caller | Temporary values. Dead at a call boundary. | Trap asm uses them as scratch. |
| x8 | s0 / fp | Callee | First saved register. Also the frame pointer in code with frames. | First task entry reads the trap frame address from it. |
| x9 | s1 | Callee | Second saved register. | User program counts its loop in it. |
| x10-x17 | a0-a7 | Caller | Function arguments. `a0` and `a1` also return values. | Trap frame stores them. Syscalls: `a7` = number, `a0`-`a2` = arguments, `a0` = result. Task entry: `a0` = letter, `a1` = scratch page. |
| x18-x27 | s2-s11 | Callee | Saved registers. | User program keeps its letter and scratch address in `s0`-`s2`. |

## Supervisor control and status registers

| Register | Purpose in RISC-V | Use in this kernel |
|---|---|---|
| stvec | Trap vector. The address the hart jumps to on an S-mode trap. Must be 4-byte aligned. | Holds `_kerneltrapvec`. |
| sstatus | Status flags of S-mode. See the bit table below. | Trap path selection (SPP), interrupt state (SIE, SPIE), user memory access (SUM). |
| sie | Interrupt enable bitmap for S-mode. | Bit 9: enable the supervisor external interrupt (SEIE). |
| sepc | Program counter (PC) of the instruction that trapped. `sret` copies it back to the PC. | Trap entry writes it, `usertrap` adds 4 after an `ecall`. |
| scause | Trap cause. Bit 63: 1 = interrupt, 0 = exception. Bits 7:0: code. 5 = timer, 8 = environment call from user mode, 9 = external, 12/13/15 = page faults. | The handlers dispatch on it. |
| stval | Trap value. For page faults, the faulting virtual address. | The fault report prints it. |
| satp | Address translation control for S-mode. See the field table below. | `paging::activate` writes it. Step 28 adds the address space identifier (ASID). |
| sscratch | One scratch word for trap handling. The kernel owns the content. | Holds the kernel stack top while a task runs in user mode. `csrrw sp, sscratch, sp` swaps the stacks at trap entry. |
| time | Read-only free-running timer counter. | Timer interval arithmetic. |
| stimecmp | Timer compare value. The timer interrupt fires when `time` reaches it. | `set_timer_interval_for_interrupt` writes it. Writing it from S-mode needs the STCE bit in `menvcfg`. |

### sstatus bits used

| Bit | Name | Meaning |
|---|---|---|
| 1 | SIE | Supervisor interrupt enable. 1 = the hart takes interrupts in S-mode. Trap entry saves it into SPIE and clears it. |
| 5 | SPIE | Previous SIE. `sret` copies it into SIE. |
| 8 | SPP | Previous privilege. 1 = trap came from S-mode, 0 = user mode. `sret` selects the return privilege with it. |
| 18 | SUM | Supervisor user memory. 1 = S-mode may read and write pages with the U bit. 0 = the access traps. |

### satp fields, Sv39

| Bits | Field | Meaning |
|---|---|---|
| 63:60 | Mode | 8 = Sv39 paging on. 0 = bare, no translation. |
| 59:44 | ASID | Address space identifier. Tags all translations of one address space. |
| 43:0 | PPN | Physical page number of the root table, the level-2 table. |

## Machine control and status registers

Machine mode is the highest privilege. The boot code in `main.rs` runs in machine mode, then hands over to S-mode. Machine registers are not writable from S-mode.

| Register | Purpose in RISC-V | Use in this kernel |
|---|---|---|
| mhartid | Read-only hart number. | Parks all harts but hart 0. `utils::current_hart_id` reads it. |
| mstatus | Status flags of machine mode. Bits 12:11 (MPP) hold the privilege level that `mret` enters. | Handover sets MPP to S-mode, then `mret` drops the privilege. |
| mepc | PC target for `mret`. | Handover points it at `main`. |
| medeleg | Exception delegation bitmap. Bit n set: exception code n traps into S-mode, not machine mode. | Handover writes 0xffff. This sends page faults and `ecall` from user mode to S-mode. |
| mideleg | Interrupt delegation bitmap. Same rule for interrupt codes. | Handover writes 0xffff. This sends the timer and external interrupts to S-mode. |
| mie | Interrupt enable bitmap for machine mode. Bit 5: supervisor timer interrupt (STIE). | `timer_init` sets bit 5. The interrupt then arrives through delegation. |
| mcounteren | Counter access bitmap. Bit 1 (TM): S-mode may read the `time` CSR. | `timer_init` sets bit 1. |
| menvcfg | Environment configuration. Bit 62: page-based memory types (PBMTE). Bit 63: `stimecmp` write access from S-mode (STCE). | `paging::init` sets bit 62 for svpbmt. `timer_init` sets bit 63. |
| pmpaddr0 | Physical memory protection, entry 0. Holds a top address. | Handover writes 0x3f_ffff_ffff_ffff. |
| pmpcfg0 | Configuration of entry 0. Bits 0-2: read, write, execute. Bits 3-4: match mode. | Handover writes 0xf: read, write, execute, top-of-range (TOR). The entry opens all physical memory to S-mode. |

## Device registers, platform-level interrupt controller

The PLIC routes device interrupts to harts. Each combination of hart and privilege level owns one context, a private register window.

| Register | Purpose | Use in this kernel |
|---|---|---|
| priority (offset 0) | Priority of one interrupt source. 0 = never interrupt. | `program` writes 1 for the UART source. |
| enable (offset 0x2000) | Bitmap of sources for one context. | `program` sets the UART bit. |
| threshold (offset 0x20_0000) | Minimum priority that signals the hart. | `program` writes 0. |
| claim / complete (offset 0x20_0004) | Read: claim the highest pending source, 0 = none. Write: end its service. | `claim` and `complete` in every external interrupt. |

## Device registers, universal asynchronous receiver-transmitter

A 16550-compatible UART. Bit 7 of the line control register (LCR), the divisor latch access bit (DLAB), selects the meaning of offsets 0 and 1: 0 = data registers, 1 = divisor latches.

| Register | Offset | Direction | Purpose | Use in this kernel |
|---|---|---|---|---|
| Transmitter holding register (THR) | 0 | Write | One byte to send. | `send_byte`. |
| Receiver buffer register (RBR) | 0 | Read | One byte received. | `handle_interrupt`. |
| Divisor latch low (DLL), high (DLM) | 0, 1 | Write | Clock divisor for the baud rate. | `set_baude_rate` writes 3 and 0. |
| Interrupt enable register (IER) | 1 | Write | Interrupt masks: bit 0 received data, bit 1 holding empty, bit 2 line status. | Receive and line status enabled. |
| Interrupt identification register (IIR) | 2 | Read | Which interrupt fired. | `handle_interrupt` dispatches on it. |
| Line control register (LCR) | 3 | Write | Data bits, stop bits, parity, DLAB. | 8 data bits, no parity, 1 stop bit. DLAB toggling for the divisor. |
| Line status register (LSR) | 5 | Read | Line state and errors. Reading it clears the line status interrupt. | Error path reads it. |
