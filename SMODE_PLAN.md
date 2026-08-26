# S-mode Plan (step 4)

Goal: M-mode sets up, delegates, drops to S-mode. Echo + timer tick both run on the S-mode trap path.

Two chains now. Each arrow needs one enable:

```
UART (IER) → PLIC (ctx 1) → sie.SEIE → sstatus.SIE → stvec → kernelvec → kerneltrap
CLINT (mtimecmp) ────────→ sie.STIE ──┘
```

You write the code. This file gives the steps, addresses, and bit numbers only.

## Step 0 — `plic.rs`: new context

qemu virt contexts: hart `i` M-mode = `2*i`, S-mode = `2*i + 1`.

1. Rename `m_mode_ctx` → `s_mode_ctx`: `2 * hart_id + 1`.
2. init/claim/complete use it. Addresses, bits, order: unchanged.

New numbers for ctx 1 (needed in the debug list below):

| Register | Address |
|---|---|
| Enable | `0x0C00_2080` |
| Threshold | `0x0C20_1000` |
| Claim / complete | `0x0C20_1004` |

## Step 1 — `start()`: delegate, then drop

After `uart::init()` and `plic::init()`, in **this order**:

1. Remove `kernel_trap::kernel_trap_init()`. `mtvec`/`mie.MEIE`/`mstatus.MIE` are M-mode machinery. Step 5 replaces them.
2. `csrw medeleg, 0xffff` — exceptions go to S-mode.
3. `csrw mideleg, 0xffff` — interrupts go to S-mode.
4. PMP — without it S-mode touches no memory:
   - `pmpaddr0 = 0x3fffffffffffff`
   - `pmpcfg0 = 0xf` (R|W|X, A=TOR → covers everything)
5. `csrw satp, 0` — no paging yet.
6. `csrw mepc, main` — address of your new S-mode entry.
7. `mstatus`: clear bits 12:11 (MPP), then set MPP = `01` (S). Read-modify-write.
8. Save `mhartid` in `tp`. S-mode cannot read `mhartid`. The timer handler needs it for `mtimecmp(hart)`.
9. `timer_init()` (step 2).
10. `mret` — execution continues at `main`, in S-mode. `start()` does not return.

## Step 2 — `timer_init()` (CLINT)

| Register | Address | Access |
|---|---|---|
| `mtime` | `0x200_BFF8` | u64, read |
| `mtimecmp`, hart `i` | `0x200_4000 + 8*i` | u64, read-write |

1. `mtimecmp(hart) = mtime + 1_000_000`. qemu CLINT: 10 MHz → 0.1 s per interval.
2. Set `mie.STIE` (bit 5). `mideleg` bit 5 is set, so this same bit is visible as `sie.STIE` later.

Later xv6 uses the Sstc extension (`stimecmp`) instead of CLINT. Skip for now.

## Step 3 — kernelvec asm: one edit

Same frame, same save/restore, same `.align 2`. One change only:

- `mret` → `sret`.

## Step 4 — `kerneltrap`: S-mode edition

Signature unchanged.

1. `csrr scause` (was `mcause`).
2. Interrupt bit = 63. New codes:

   | code | source |
   |---|---|
   | `9` | supervisor external → PLIC |
   | `5` | supervisor timer → CLINT |
3. Code 9: same as before — claim, `irq == 10` → `uart::handle_interrupt()`, `irq != 0` → complete.
4. Code 5: first `mtimecmp(tp) += 1_000_000` (re-arm before the work — no drift), then tick work. For now: count ticks in a static, `send_byte(b'.')` every 10th.
5. Else: spin, same as before.

`mtimecmp` is memory-mapped. The PMP write from step 1 lets S-mode use it directly.

## Step 5 — `main()`: enables last

New `extern "C"` function. `mepc` points here. In **this order**:

1. `csrw stvec, kernelvec` — low 2 bits `00` = direct mode.
2. `csrs sie, (1<<9) | (1<<5)` — SEIE | STIE (`= 0x220`).
3. `csrs sstatus, 1<<1` — SIE.
4. Same loop as before: `pop_byte()` → `send_byte()` → else `wfi`. (`wfi` is legal in S-mode.)

If you set the enables before `stvec` is valid, a keypress traps into garbage. Same rule as M-mode.

## Test

1. `cargo run`.
2. Type characters. They must echo — proves code 9 + PLIC ctx 1.
3. A dot appears ~1/s — proves code 5 + re-arm.
4. Nothing at all, check in order:
   - `stvec` set? `sstatus.SIE` set? `sie` = `0x220`?
   - PLIC: enable bit at `0x0C00_2080`? Threshold at `0x0C20_1000` = 0?
   - `medeleg`/`mideleg` = `0xffff`, written **before** `mret`?
   - PMP written? Without it the first S-mode access traps into your spin loop, silently.
   - kernelvec ends in `sret`, not `mret`?
5. Echo works, no dots:
   - `mie.STIE` set before `mret`?
   - `mtimecmp` at `0x200_4000 + 8*hartid`, written as one u64?
   - Handler re-arms `mtimecmp`? A one-shot tick fires exactly once.

## Cleanup

After the test passes:

- Delete `kernel_trap_init()` and the M-mode cause constant (11).
- `uart.rs` and `plic.rs` logic unchanged — only the ctx number moved.

## Later — step 5 (kalloc)

Freelist over `end` → `PHYSTOP`. Then add `alloc`. The ring buffer and tick counter become `Vec`/`Box` candidates.
