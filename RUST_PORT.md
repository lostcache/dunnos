I'm using you to learn OS and working on rust port of xv6 os from mit to do so. so your job is to make sure I learn stuff without too much help

# xv6 → Rust Port

Pure Rust rewrite of xv6-riscv. No C kernel code. Asm only for: `_entry`, trampoline, `swtch`.

## Rules

- One milestone boots before the next starts.
- `core` only until `kalloc` done → then add `alloc` (Vec/String/Box). Never `std`.
- Verify each step with `cargo run` (qemu).

## Roadmap

- [x] **1. Setup** — nightly + `riscv64gc-unknown-none-elf`, `build-std=["core"]`, builds clean
- [x] **2. Boot** — `kernel.ld`, `_entry` asm (per-hart stacks), `start()` spins
- [x] **3. UART** — NS16550A @ `0x1000_0000`, ring buffer, echo loop. PLIC + M-mode trap chain done early (see INTERRUPT_PLAN.md)
- [ ] **4. start (M-mode)** — timer, delegate traps to S-mode, `mret` → `main`. Then redo interrupt chain in S-mode (stvec/sie/scause 9, PLIC ctx 1). See SMODE_PLAN.md
- [ ] **5. Page allocator** — `kalloc.rs`: freelist, `end`→`PHYSTOP`; then `alloc`
- [ ] **6. VM** — Sv39 walk/map, kernel page table, satp + sfence
- [ ] **7. Traps** — `trap.rs`, kernelvec + trampoline asm (trampoline at fixed VA, page-aligned)
- [ ] **8. Processes** — proc table, context, `swtch` (naked_asm), scheduler
- [ ] **9. Syscalls** — fork/exec/exit/wait/write → run `init`
- [ ] **10. Devices** — `plic.rs`, `virtio_disk.rs` (DMA ring, `read_volatile`)
- [ ] **11. FS** — bio → log → fs → file/pipe → exec
- [ ] **12. Userland** — reuse C binaries + `mkfs` first; port to Rust last (optional)

## Files so far

| File | Purpose |
|---|---|
| `rust-toolchain.toml` | nightly, rust-src, riscv64gc target |
| `Cargo.toml` | `panic = "abort"` both profiles |
| `.cargo/config.toml` | target, `build-std`, linker arg, qemu runner |
| `kernel.ld` | sections @ `0x80000000`, `PROVIDE(end)` |
| `src/main.rs` | `_entry` asm + `start()` + `#[panic_handler]` |

## Key decisions / gotchas

- **nightly needed**: `build-std`, `global_asm!`, `naked_asm!` are unstable
- **`rust-src`** = source of `core`; `build-std` = instruction to compile it
- **`panic = "abort"`**: no unwinding runtime in a kernel
- **entry asm**: only job = per-hart stack via `mhartid`, then `call start`
- **trampoline must be page-aligned** and mapped at same VA in kernel + user tables (step 7)
- **`ALIGN(4K)` in linker script** = for MMU page permissions (step 6), not today
- qemu exit: `Ctrl+A` then `X`

## Current position

UART + PLIC + kernel trap (M-mode) done; keypress echoes via interrupt + `wfi`. **Next: step 4 — M→S transition + timer.**
