# Plan: steps 22 to 27 — user mode, tasks, context switch

Details steps 22 to 27 of `plan.md`. All references point to files in this repo and to the Linux kernel. No other knowledge is required.

## Where you start from (facts)

- `main()` runs in supervisor mode (S-mode). It activates paging, prints `P`, tests `Box`, then loops: UART input, else `wfi`.
- `_kerneltrapvec` in `kernel_trap.rs` saves x1 and x3 to x31 at stack slots, calls `kerneltrap()`, restores, then `sret`. `kerneltrap()` reads `scause` from the control and status register (CSR) and handles codes 5 (timer) and 9 (external). Codes 12, 13, 15 print a fault line, then park. All other codes spin.
- This step keeps that path. The kernel path returns on the same stack, never switches task, and needs no trap frame. Only the values that a return or a switch would lose go into a frame.
- `paging.rs`: `map_virtual_to_physical(root, va, pa, flags)` forces the global bit on every leaf. The code carries the note: remove this for ASID != 0. The `USER` flag constant exists, unused.
- `mm.rs`: `alloc_frame()` returns a zeroed frame, `free_frame()`, `mark_used()`.
- Entry asm gives one 4 KiB stack per hart from `stack0`. Non-boot harts park. `tp` holds the hart ID.
- Output markers today: `F+HpP+`, then one dot per 10 timer ticks.

## Terms

| Term | Meaning |
|---|---|
| U-mode | User mode. The lowest privilege level of the hart. |
| ecall | Environment call. The instruction that traps from U-mode to S-mode. Cause code 8. |
| Trap frame | The register set saved at trap entry. Linux calls it `pt_regs` (`arch/riscv/include/asm/ptrace.h`). |
| Context switch | Save the callee-saved registers of one task, then load the set of another task. |
| SPP / SPIE | sstatus bits: previous privilege, previous interrupt enable. `sret` reads them. |
| SUM | Supervisor user memory. sstatus bit 18. Set: S-mode code can read and write pages with the U bit. |
| ASID | Address space identifier. The 16-bit field at satp bits 59:44. |
| Preemption | A task switch forced by the timer, not by a yield call. |
| Reap | Free the memory of a dead task, from another task's context. |

## Design, fixed

- One hart. Per-hart arrays come with the symmetric multiprocessing (SMP) work.
- Task table: static array of 16 slots. Slot 0 is the boot context, always schedulable.
- Each task holds: one 4 KiB kernel stack, one root table, four user frames (code, scratch, two stack pages).
- The kernel identity map lives inside every task root table. Kernel leaves carry the global bit, so they stay valid across a switch.
- User pages live at virtual addresses above the highest physical address. One layout for all tasks: code, scratch, stack. Each task has its own root, so the addresses do not clash.
- The trap frame of a user task sits at a fixed slot: kernel stack top minus frame size. It survives a context switch.
- Two trap vectors, selected by `stvec`. `_kerneltrapvec` serves S-mode traps and keeps the current stack. `_usertrapvec` serves U-mode traps and switches to the task kernel stack first. Linux and xv6 split the paths the same way (`arch/riscv/kernel/entry.S`). A single vector would need a scratch register to test the mode, and that register holds a user value at that point.
- The frame holds only what the return adds back and what a switch clobbers: the 31 registers, `sstatus`, `sepc`. The cause and the fault address change no code and stay in their CSRs. `usertrap` reads them at entry, while the values are still live.
- Syscall numbers match Linux: write 64, exit 93, sched_yield 124 (`arch/riscv/include/uapi/asm/unistd.h`).
- Supervisor interrupts stay off inside a trap handler. The hardware clears the SIE bit on trap entry. No nested frame is required.

## Step 22 — `task.rs`: context switch and round robin

**Purpose.** Give the kernel tasks it can switch between, before user mode exists.

**Background.**

- `switch_to` saves the callee-saved set: `ra`, `sp`, `s0` to `s11`. The caller-saved set is dead at function boundaries. Linux saves the same registers in `arch/riscv/kernel/process.c:__switch_to`.
- The boot context becomes task 0. A switch to task 0 returns into the old `schedule()` call. Linux does the same: the idle task is a normal task (`kernel/sched/core.c`).
- A new task starts at `ra` with a fresh `sp`. Linux uses `ret_from_fork` (`arch/riscv/kernel/entry.S`) for the same purpose.

**Change 1. Create `src/task.rs`.** Add `mod task;` to `main.rs`.

```rust
#[derive(PartialEq)]
pub(crate) enum TaskState {
    Unused,
    Ready,
    Running,
    Dead,
}

/// Callee-saved set of a task. `switch_to` asm fills and drains it.
#[repr(C)]
pub(crate) struct TaskContext {
    ra: usize,
    sp: usize,
    s0: usize,
    s1: usize,
    s2: usize,
    s3: usize,
    s4: usize,
    s5: usize,
    s6: usize,
    s7: usize,
    s8: usize,
    s9: usize,
    s10: usize,
    s11: usize,
}

pub(crate) struct Task {
    ctx: TaskContext,
    /// Kernel stack top. 0 for the boot task.
    kstack_top: usize,
    /// Root table for `satp`. The kernel root for the boot task.
    root_table: usize,
    pid: usize,
    /// Step 27. 0 for the boot task.
    asid: usize,
    /// User frames to free at reap. Step 23.
    user_frames: [usize; 8],
    state: TaskState,
}
```

**Change 2. `switch_to` asm.**

```rust
global_asm!(
    r#"
    .align 2
    .global switch_to
switch_to:                   # a0 = old context, a1 = new context.
    sd ra, {ra}(a0)
    sd sp, {sp}(a0)
    sd s0, {s0}(a0)
    sd s1, {s1}(a0)
    sd s2, {s2}(a0)
    sd s3, {s3}(a0)
    sd s4, {s4}(a0)
    sd s5, {s5}(a0)
    sd s6, {s6}(a0)
    sd s7, {s7}(a0)
    sd s8, {s8}(a0)
    sd s9, {s9}(a0)
    sd s10, {s10}(a0)
    sd s11, {s11}(a0)
    ld ra, {ra}(a1)
    ld sp, {sp}(a1)
    ld s0, {s0}(a1)
    ld s1, {s1}(a1)
    ld s2, {s2}(a1)
    ld s3, {s3}(a1)
    ld s4, {s4}(a1)
    ld s5, {s5}(a1)
    ld s6, {s6}(a1)
    ld s7, {s7}(a1)
    ld s8, {s8}(a1)
    ld s9, {s9}(a1)
    ld s10, {s10}(a1)
    ld s11, {s11}(a1)
    ret
    "#,
    ra = const offset_of!(TaskContext, ra),
    sp = const offset_of!(TaskContext, sp),
    s0 = const offset_of!(TaskContext, s0),
    s1 = const offset_of!(TaskContext, s1),
    s2 = const offset_of!(TaskContext, s2),
    s3 = const offset_of!(TaskContext, s3),
    s4 = const offset_of!(TaskContext, s4),
    s5 = const offset_of!(TaskContext, s5),
    s6 = const offset_of!(TaskContext, s6),
    s7 = const offset_of!(TaskContext, s7),
    s8 = const offset_of!(TaskContext, s8),
    s9 = const offset_of!(TaskContext, s9),
    s10 = const offset_of!(TaskContext, s10),
    s11 = const offset_of!(TaskContext, s11),
);

pub(crate) unsafe extern "C" {
    fn switch_to(old: *mut TaskContext, new: *const TaskContext);
}
```

**Change 3. State and `schedule()`.**

```rust
static TASKS: SyncUnsafeCell<[Task; MAX_TASKS]> =
    SyncUnsafeCell::new([const { Task::empty() }; MAX_TASKS]);
static CURRENT: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);

/// Kernel stack top of the running task. The user trap asm reads it.
#[unsafe(no_mangle)]
pub(crate) static CURRENT_KSTACK_TOP: SyncUnsafeCell<usize> = SyncUnsafeCell::new(0);
```

```rust
/// Picks the next ready task after the current one, then switches.
/// Returns at once when the boot task runs and no task is ready.
pub(crate) fn schedule() {
    let cur = unsafe { *CURRENT.get() };
    let next = pick_next(cur); // See below.
    if next == cur {
        return;
    }
    if task_state(cur) == TaskState::Running {
        set_state(cur, TaskState::Ready);
    }
    set_state(next, TaskState::Running);
    unsafe {
        *CURRENT.get() = next;
        *CURRENT_KSTACK_TOP.get() = task(next).kstack_top;
        paging::activate(task(next).root_table);
        switch_to(core::ptr::addr_of_mut!(task_mut(cur).ctx), &task(next).ctx);
    }
    // Control is back in this context. Another schedule call has set
    // CURRENT, CURRENT_KSTACK_TOP and satp for this task already.
}
```

`pick_next(cur)`: scan slots `cur + 1` to the end, then 1 to `cur`, for `Ready`. No hit and `cur != 0` → 0 (boot). No hit and `cur == 0` → `cur` (nothing to do).

`init(kernel_root, boot_kstack_top)` fills slot 0: state `Running`, `root_table = kernel_root`, `pid = 0`. Read `boot_kstack_top` from the `stack0` symbol plus 4096, the `mm.rs` pattern.

**Change 4. Call order in `main()`.** After the `Box` marker, before the idle loop:

```rust
task::init(root, boot_kstack_top);
```

**Self-test, temporary.** Add a test slot: one frame from `mm::alloc_frame()` as kernel stack, `ctx.ra` = a test function address, `ctx.sp` = stack top minus 16, state `Ready`. The test function sends `S`, marks its slot `Dead`, then calls `schedule()`. In `main()`, after `task::init`, call `task::schedule()`.

**Verify.**

1. `cargo run` shows `F+HpP+S`, then dots.
2. Remove the self-test. `cargo run` shows `F+HpP+` plus dots.
3. `cargo build` and `cargo clippy` print no warnings.

## Step 23 — user pages and `spawn()`

**Purpose.** Create the address space of a task, without running it yet.

**Background.**

- Every task root table holds the kernel identity map plus the user pages. The table is a supervisor structure: the `U` bit on a leaf decides what U-mode may touch. This is the Linux model: one page table per task, kernel half and user half (`arch/riscv/mm/init.c`).
- User pages must not clash with the identity map. They live above the highest physical address.
- The global bit belongs on kernel leaves only. `map_virtual_to_physical` must stop forcing it. Kernel map flags already carry it (`IDENTITY_MAP_FLAGS`, `MMIO_FLAGS`).
- `alloc_frame()` returns zeroed frames, so the code page needs no extra clear.

**Change 1. `paging.rs`: drop the forced global bit.** In `map_virtual_to_physical`, write `flags` without `| GLOBAL`. The kernel callers pass the bit through their flag constants.

**Change 2. `paging.rs`: share the kernel map.** Move the body of `init()` after the root allocation into:

```rust
fn map_kernel_regions(root: usize, mmio_flags: u64) -> Result<(), PagingError> {
    for range in mm::get_physical_addr_ranges() {
        map_identity_range(root, range.start, range.end, IDENTITY_MAP_FLAGS)?;
    }
    map_mmio_device(root, b"ns16550a", mmio_flags)?;
    map_mmio_device(root, b"riscv,plic0", mmio_flags)?;
    Ok(())
}
```

Add:

```rust
/// Fresh root with the kernel map inside. Supervisor mode.
pub(crate) fn new_task_root() -> Result<usize, PagingError> {
    let mut mmio_flags = MMIO_FLAGS;
    if svpbmt_supported() {
        mmio_flags |= PHYSICAL_MEMORY_TYPE_INPUT_OUTPUT;
    }
    let root = mm::alloc_frame().map_err(|_| PagingError::OutOfFrames)?;
    map_kernel_regions(root, mmio_flags)?;
    Ok(root)
}

const USER_CODE_FLAGS: u64 = READ | EXECUTE | USER;
const USER_DATA_FLAGS: u64 = READ | WRITE | USER;
```

**Change 3. User address layout, computed once in `task.rs`.**

```text
user_va_base = highest physical range end, rounded up to 2 MiB.
code page    = base
scratch page = base + 0x1000
stack        = base + 0x2000, two pages, user sp = base + 0x4000
```

All tasks share the layout. Separate roots make the addresses distinct.

**Change 4. The user program, embedded bytes.** In `task.rs`, with the `mm.rs` pattern for the symbols:

```rust
global_asm!(
    r#"
    .section .user, "ax"
    .align 2
    .global user_prog_start
user_prog_start:
    mv   s0, a0            # Letter byte.
    mv   s2, a1            # Scratch page address.
    li   s1, 8             # Loop count.
.Lloop:
    sb   s0, 0(s2)
    li   a7, 64            # write
    li   a0, 1
    mv   a1, s2
    li   a2, 1
    ecall
    li   a7, 124           # sched_yield. Step 26 replaces this.
    ecall
    addi s1, s1, -1
    bnez s1, .Lloop
    li   a7, 93            # exit
    li   a0, 0
    ecall
.Lpark:                    # exit must not return.
    wfi
    j .Lpark
    .global user_prog_end
user_prog_end:
"#
);

unsafe extern "C" {
    #[link_name = "user_prog_start"]
    static USER_PROG_START: u8;
    #[link_name = "user_prog_end"]
    static USER_PROG_END: u8;
}
```

The program is position independent: all addresses arrive in registers.

**Change 5. `spawn(letter)`.**

1. Find an `Unused` slot. `pid` = slot index + 1.
2. Frames from `mm::alloc_frame()`: kernel stack, code, scratch, stack page one, stack page two. Record the four user frames in `user_frames`.
3. `root = paging::new_task_root()?`.
4. Map: code page with `USER_CODE_FLAGS`, scratch and both stack pages with `USER_DATA_FLAGS`.
5. Copy the program: `ptr::copy` from `USER_PROG_START` to `USER_PROG_END` into the code frame. The kernel identity map covers both.
6. Frame at `kstack_top - size_of::<TrapFrame>()`: set `sstatus` = SPIE (bit 5) so `sret` enables interrupts in user mode, `sepc` = code page address, `x2` = stack top address, `x10` = letter, `x11` = scratch address. Leave the rest zero.
7. `ctx`: `ra` = `task_first_entry` address, `s0` = frame address, `sp` = frame address.
8. State `Ready`. Send `t` when the slot holds the task.

`TrapFrame` arrives in step 24. For this step, reserve the slot at `kstack_top - 272` and keep the size in one constant.

**Verify.** `cargo run` shows `F+HpP+tt` plus dots. No task runs yet. Clean build and clippy.

## Step 24 — the user trap vector, `sret`, and the environment call

**Purpose.** Run the user program and service its `ecall`.

**Background.**

- The kernel path does not change. `_kerneltrapvec` keeps its save and restore, and `kerneltrap()` keeps its CSR reads.
- The user path must leave the user stack before it saves a register. `sscratch` holds the kernel stack top while a task runs in U-mode. `csrrw sp, sscratch, sp` swaps the stacks and keeps the user stack pointer safe in the CSR. No other register is touched, so nothing is lost.
- A trap during a trap handler is possible. `_usertrapvec` returns `stvec` to `_kerneltrapvec` before it calls Rust. `user_trap_ret` points `stvec` back at `_usertrapvec` before `sret`.
- `sret` rules: privilege = SPP, SIE = SPIE, SPIE = 1, SPP = 0. A frame with SPP = 0 and SPIE = 1 enters U-mode with interrupts on.
- The frame needs the return state only: the 31 registers, `sstatus`, `sepc`. The size is 272 bytes, a multiple of 16.
- S-mode code cannot read U-bit pages while SUM = 0. Set SUM in the user trap handler. Linux toggles SUM around user copies (`arch/riscv/include/asm/uaccess.h`).
- `ecall` advances no `sepc`. The handler adds 4.
- A yield with one task parks the task: no other task is ready. The idle loop must poll for ready tasks.

**Change 1. `TrapFrame` in `kernel_trap.rs`.**

```rust
/// Saved context of a user task. One layout for asm and Rust.
/// `x0` is constant zero. `pad` keeps the size a multiple of 16.
#[repr(C)]
pub(crate) struct TrapFrame {
    pub(crate) x1: usize,
    pub(crate) x2: usize, // User stack pointer.
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
    pub(crate) pad: usize,
}
```

**Change 2. `_usertrapvec` in the `global_asm!` of `kernel_trap.rs`.**

```text
    .global _usertrapvec
    .align 2
_usertrapvec:
    csrrw sp, sscratch, sp     # sp = kernel stack top, sscratch = user sp.
    addi  sp, sp, -{frame}
    sd    x1, {x1}(sp)
    sd    x3, {x3}(sp)
    sd    x4, {x4}(sp)
    sd    x5, {x5}(sp)
    sd    x6, {x6}(sp)
    sd    x7, {x7}(sp)
    sd    x8, {x8}(sp)
    sd    x9, {x9}(sp)
    sd    x10, {x10}(sp)
    sd    x11, {x11}(sp)
    sd    x12, {x12}(sp)
    sd    x13, {x13}(sp)
    sd    x14, {x14}(sp)
    sd    x15, {x15}(sp)
    sd    x16, {x16}(sp)
    sd    x17, {x17}(sp)
    sd    x18, {x18}(sp)
    sd    x19, {x19}(sp)
    sd    x20, {x20}(sp)
    sd    x21, {x21}(sp)
    sd    x22, {x22}(sp)
    sd    x23, {x23}(sp)
    sd    x24, {x24}(sp)
    sd    x25, {x25}(sp)
    sd    x26, {x26}(sp)
    sd    x27, {x27}(sp)
    sd    x28, {x28}(sp)
    sd    x29, {x29}(sp)
    sd    x30, {x30}(sp)
    sd    x31, {x31}(sp)
    csrr  t1, sscratch         # User sp. x6 is safe in memory.
    sd    t1, {x2}(sp)
    csrr  t1, sstatus
    sd    t1, {sstatus}(sp)
    csrr  t1, sepc
    sd    t1, {sepc}(sp)
    la    t0, _kerneltrapvec   # A nested trap uses the kernel path.
    csrw  stvec, t0
    mv    a0, sp
    call  usertrap
    mv    a0, sp
    j     user_trap_ret
```

Add the return and first-entry labels:

```text
    # Return to user mode. a0 = trap frame address.
    .global user_trap_ret
user_trap_ret:
    ld    t0, {sstatus}(a0)
    csrw  sstatus, t0
    ld    t0, {sepc}(a0)
    csrw  sepc, t0
    la    t0, CURRENT_KSTACK_TOP
    ld    t0, 0(t0)
    csrw  sscratch, t0         # For the next trap from U-mode.
    la    t0, _usertrapvec
    csrw  stvec, t0
    ld    x1, {x1}(a0)
    ld    x3, {x3}(a0)
    ld    x4, {x4}(a0)
    ld    x5, {x5}(a0)
    ld    x6, {x6}(a0)
    ld    x7, {x7}(a0)
    ld    x8, {x8}(a0)
    ld    x9, {x9}(a0)
    ld    x10, {x10}(a0)
    ld    x11, {x11}(a0)
    ld    x12, {x12}(a0)
    ld    x13, {x13}(a0)
    ld    x14, {x14}(a0)
    ld    x15, {x15}(a0)
    ld    x16, {x16}(a0)
    ld    x17, {x17}(a0)
    ld    x18, {x18}(a0)
    ld    x19, {x19}(a0)
    ld    x20, {x20}(a0)
    ld    x21, {x21}(a0)
    ld    x22, {x22}(a0)
    ld    x23, {x23}(a0)
    ld    x24, {x24}(a0)
    ld    x25, {x25}(a0)
    ld    x26, {x26}(a0)
    ld    x27, {x27}(a0)
    ld    x28, {x28}(a0)
    ld    x29, {x29}(a0)
    ld    x30, {x30}(a0)
    ld    x31, {x31}(a0)
    ld    sp, {x2}(a0)         # User sp. Last load.
    sret

    # First entry of a new task, after switch_to.
    .global task_first_entry
task_first_entry:
    mv    a0, s0               # s0 = trap frame address, set by spawn.
    j     user_trap_ret
```

The `global_asm!` operand list binds the slot names to the struct:

```rust
    frame = const size_of::<TrapFrame>(),
    x1 = const offset_of!(TrapFrame, x1),
    x2 = const offset_of!(TrapFrame, x2),
    x3 = const offset_of!(TrapFrame, x3),
    x4 = const offset_of!(TrapFrame, x4),
    x5 = const offset_of!(TrapFrame, x5),
    x6 = const offset_of!(TrapFrame, x6),
    x7 = const offset_of!(TrapFrame, x7),
    x8 = const offset_of!(TrapFrame, x8),
    x9 = const offset_of!(TrapFrame, x9),
    x10 = const offset_of!(TrapFrame, x10),
    x11 = const offset_of!(TrapFrame, x11),
    x12 = const offset_of!(TrapFrame, x12),
    x13 = const offset_of!(TrapFrame, x13),
    x14 = const offset_of!(TrapFrame, x14),
    x15 = const offset_of!(TrapFrame, x15),
    x16 = const offset_of!(TrapFrame, x16),
    x17 = const offset_of!(TrapFrame, x17),
    x18 = const offset_of!(TrapFrame, x18),
    x19 = const offset_of!(TrapFrame, x19),
    x20 = const offset_of!(TrapFrame, x20),
    x21 = const offset_of!(TrapFrame, x21),
    x22 = const offset_of!(TrapFrame, x22),
    x23 = const offset_of!(TrapFrame, x23),
    x24 = const offset_of!(TrapFrame, x24),
    x25 = const offset_of!(TrapFrame, x25),
    x26 = const offset_of!(TrapFrame, x26),
    x27 = const offset_of!(TrapFrame, x27),
    x28 = const offset_of!(TrapFrame, x28),
    x29 = const offset_of!(TrapFrame, x29),
    x30 = const offset_of!(TrapFrame, x30),
    x31 = const offset_of!(TrapFrame, x31),
    sstatus = const offset_of!(TrapFrame, sstatus),
    sepc = const offset_of!(TrapFrame, sepc),
```

**Change 3. `usertrap` in `kernel_trap.rs`.** The cause and the fault address stay in their CSRs. Read them here, at entry.

```rust
const ENV_CALL_FROM_USER_MODE: usize = 8;

#[unsafe(no_mangle)]
unsafe extern "C" fn usertrap(frame: *mut TrapFrame) {
    let f = unsafe { &mut *frame };
    let scause: usize;
    let stval: usize;
    unsafe {
        asm!("csrr {0}, scause", out(reg) scause);
        asm!("csrr {0}, stval", out(reg) stval);
        asm!("csrs sstatus, {0}", in(reg) 1usize << 18); // SUM
    }
    let is_interrupt = scause >> 63 != 0;
    let code = scause & 0xff;
    if !is_interrupt && code == ENV_CALL_FROM_USER_MODE {
        f.sepc += 4;
        syscall(f);
    } else if is_interrupt && code == TIMER_INTERRUPT {
        timer_interrupt::handle_timer_interrupt();
    } else if is_interrupt && code == SUPERVISOR_EXTERNAL_INTERRUPT_CODE {
        let irq = plic::claim();
        if irq == uart::irq() {
            uart::handle_interrupt();
        }
        if irq != 0 {
            plic::complete(irq);
        }
    } else {
        report_fault(scause, stval, f.sepc); // Step 27 kills the task instead.
    }
}
```

`report_fault` prints the line and parks, the same body as today's `report_page_fault`.

**Change 4. `syscall` dispatch.** Linux numbers: write 64, exit 93, sched_yield 124.

```rust
fn syscall(f: &mut TrapFrame) {
    match f.x17 {
        SYS_WRITE => {
            let buf = f.x11 as *const u8;
            let len = f.x12.min(256);
            for i in 0..len {
                let byte = unsafe { core::ptr::read_volatile(buf.add(i)) };
                uart::send_byte(byte);
            }
            f.x10 = len;
        }
        SYS_EXIT => task::kill_current(),        // Never returns.
        SYS_SCHED_YIELD => {
            f.x10 = 0;
            task::schedule();
        }
        _ => f.x10 = (-38isize) as usize,       // Linux -ENOSYS.
    }
}
```

**Change 5. `kill_current` in `task.rs`.**

```rust
/// Marks the running task dead, then switches away. The reaper in
/// schedule frees the memory. This code runs on the task stack, so
/// it must not free anything itself.
pub(crate) fn kill_current() -> ! {
    let cur = unsafe { *CURRENT.get() };
    set_state(cur, TaskState::Dead);
    schedule();
    // schedule never returns into a dead task.
    loop {
        unsafe { asm!("wfi"); }
    }
}
```

**Change 6. `main()`: spawn one task, poll in the idle loop.**

```rust
if task::spawn(b'A').is_ok() { /* spawn sent t */ }

loop {
    if task::has_ready() {
        task::schedule();
        continue;
    }
    match uart::pop_byte() {
        Some(byte) => uart::send_byte(byte),
        None => unsafe {
            asm!("wfi");
        },
    }
}
```

`has_ready()` returns true when any slot holds `Ready`.

**Verify.**

1. `cargo run`. Expected stream: `F+HpP+t` + `AAAAAAAA` (8 letters, one per write), then dots. Each yield moves control to the idle loop, the poll moves it back.
2. Clean build and clippy.

## Step 25 — two tasks, cooperative round robin

**Purpose.** Prove the switch between two user address spaces.

**Background.** Linux picks the next task in `kernel/sched/core.c:pick_next_task`. This kernel scans the table in order.

**Change.** In `main()`, spawn two tasks:

```rust
task::spawn(b'A');
task::spawn(b'B');
```

**Verify.** `cargo run`. Expected stream: `F+HpP+tt` + `ABABABAB`, then dots. Each write, then a yield, moves control to the other task. Clean build and clippy.

## Step 26 — timer preemption

**Purpose.** Switch tasks from the timer, without a yield call.

**Background.** A timer trap from U-mode finds the task in a saved state. The handler may switch: the frame sits at the task kernel stack top and survives the switch. Linux preempts on the return to user mode (`kernel/sched/core.c`).

**Change 1. `usertrap`: preempt on the timer.**

```rust
} else if is_interrupt && code == TIMER_INTERRUPT {
    timer_interrupt::handle_timer_interrupt();
    task::schedule(); // Preempt.
}
```

**Change 2. User program: replace the yield.**

```text
    # Replaces: li a7, 124 + ecall.
    li    t0, 5_000_000
.Ldelay:
    addi  t0, t0, -1
    bnez  t0, .Ldelay
```

**Verify.** `cargo run`. Expected stream: `F+HpP+tt` + letters of both tasks in turn, timer driven, then dots. The letters may group as `AABAB` and similar: the switch happens at the next tick. Clean build and clippy.

## Step 27 — ASID, teardown, fault kill

**Purpose.** Make the switch cheap and the exit complete, and stop a user fault from parking the kernel.

**Background.**

- `satp` holds the ASID at bits 59:44. `sfence.vma` with a register two value flushes only that ASID. Kernel leaves carry the global bit, so an ASID-local flush keeps them valid.
- Linux manages ASIDs in `arch/riscv/mm/context.c`. This kernel assigns ASID = pid, at most 15, and flushes the ASID when the task dies. The plan assumes at least 4 ASID bits.
- The reaper must not free the stack it runs on. It runs in the context of the caller of `schedule()`, never in a dead task.
- Linux frees page tables with a table walk in `mm/memory.c:free_pgtables`.

**Change 1. `paging.rs`: ASID and table free.**

```rust
pub(crate) fn activate_with_asid(root_table: usize, asid: usize) {
    let satp = ((root_table >> PAGE_SHIFT) as u64)
        | SUPERVISOR_ADDRESS_TRANSLATION_AND_PROTECTION_MODE_SV39
        | ((asid as u64) << 44);
    unsafe {
        asm!("csrw satp, {0}", in(reg) satp);
        asm!("sfence.vma zero, {0}", in(reg) asid);
    }
}

/// Frees every table frame of the root. Leaf targets belong to the
/// task frame list, so the walk frees tables only.
pub(crate) fn free_task_root(root: usize) {
    free_table(root, 2);
    mm::free_frame(root);
}

fn free_table(table: usize, level: usize) {
    for index in 0..PAGE_TABLE_ENTRIES_PER_PAGE {
        let entry = read_page_table_entry(table, index);
        if let PageTableEntryKind::NextTablePointer = classify_page_table_entry(entry) {
            let next = page_table_entry_to_physical_address(entry);
            free_table(next, level - 1);
            mm::free_frame(next);
        }
    }
}
```

**Change 2. `schedule()`: ASID switch, reap.** Replace the `paging::activate` call:

```rust
paging::activate_with_asid(task(next).root_table, task(next).asid);
```

Set `asid = pid` in `spawn()`. After the `switch_to` call returns:

```rust
reap_dead();
```

```rust
/// Frees the memory of every dead task. Safe: no dead task can run.
fn reap_dead() {
    for slot in 0..MAX_TASKS {
        if task_state(slot) != TaskState::Dead {
            continue;
        }
        let t = task(slot);
        unsafe { asm!("sfence.vma zero, {0}", in(reg) t.asid) };
        for frame in t.user_frames {
            if frame != 0 {
                mm::free_frame(frame);
            }
        }
        paging::free_task_root(t.root_table);
        mm::free_frame(t.kstack_top - mm::PAGE_SIZE);
        set_state(slot, TaskState::Unused);
    }
}
```

**Change 3. `usertrap`: a fault kills the task.**

```rust
} else {
    let pid = task::current_pid();
    uart::send_str(b"\nFAULT task=");
    uart::send_hex(pid);
    uart::send_str(b" scause=");
    uart::send_hex(scause);
    uart::send_str(b" sepc=");
    uart::send_hex(f.sepc);
    uart::send_str(b" stval=");
    uart::send_hex(stval);
    uart::send_str(b"\n");
    task::kill_current();
}
```

Supervisor-mode faults still park: `kerneltrap` keeps `report_page_fault`.

**Verify.**

1. `cargo run`. Normal stream: `F+HpP+tt` + letters of both tasks, then dots.
2. Self-test, temporary. In the user program, before the exit call, add `li t0, <user_va_base + 64 MiB>` then `jr t0`. The address has no mapping. Expected: a `FAULT task=` line with scause 12, then dots from the other task. Remove the lines.
3. Clean build and clippy. This closes `plan.md` steps 22 to 27.

## Gates and final state

| After step | Command | Must show |
|---|---|---|
| 22 | `cargo run` | self-test prints `S`; after removal `F+HpP+` + dots |
| 23 | `cargo run` | `F+HpP+tt` + dots |
| 24 | `cargo run` | `F+HpP+t` + `AAAAAAAA` + dots |
| 25 | `cargo run` | `F+HpP+tt` + `ABABABAB` + dots |
| 26 | `cargo run` | `F+HpP+tt` + letters of both tasks, timer driven |
| 27 | `cargo run` | normal stream; self-test prints `FAULT task=` and dots continue |

Every gate also requires clean `cargo build` and `cargo clippy` output. All self-test lines are temporary. Remove each one at its gate.

## Follow-up work, not in this phase

- Heap: merge of adjacent free blocks, alignment above 16.
- User copy validation with length checks, not trust. Linux keeps this in `arch/riscv/include/asm/uaccess.h`.
- Argument vector and executable and linkable format (ELF) loading.
- Sleep and blocking: a task that waits, not spins.
- Multiple harts. The entry asm carries the `TODO(SMP)` note.
- A kernel half above the user half, one root per hart for the kernel. This is the Linux layout.
- ASID recycling with a version count, as in `arch/riscv/mm/context.c`.
