# Plan: memory map + Sv39 paging

Why Sv39:

- Mandatory RV64 paging mode. Sv48/Sv57 are optional. Linux `arch/riscv/mm/init.c:set_satp_mode()` uses the widest supported mode and treats Sv39 as the minimum.
- 3 levels = fewer page-table frames and less walk code.
- 512 GiB VA range covers the identity map: RAM at `0x8000_0000`, PLIC at `0x0c00_0000`, UART at `0x1000_0000`.
- Bare mode blocks U-mode isolation and per-process address spaces. Sv48/Sv57 can come later; the PTE format does not change.

## Work items, in order

1. Done - `fdt.rs`: store `total_size` and add `total_size()`.
2. Done - `fdt.rs`: add `reserved_regions() -> &[Resource]` (arena data already parsed, unused).
3. Done - `fdt.rs`: add `find_memory_node_idx()` for `device_type == "memory"` (Linux `early_init_dt_scan_memory()`).
4. Done - `main.rs`: park non-boot harts in `start()` (`csrr mhartid`; != 0 → `wfi` loop). Shared statics are not SMP-safe.
5. Done - `mm.rs`: kernel image range from `_entry`..`end`.
6. Done - `mm.rs`: usable RAM = memory `reg` − DTB rsvmap − DTB blob (`dtb_ptr`..`+total_size`) − kernel image (Linux `early_init_fdt_reserve_self()`).
7. Done - `mm.rs`: bitmap frame allocator, 4 KiB frames. Size the bitmap from the usable ranges in `init()`, index frames relative to the lowest usable address, place its page-aligned bytes in usable RAM, then never free them.
8. Done - `mm.rs`: APIs `init()`, `alloc_frame()`, `free_frame()`, `mark_used(range)`. Zero every allocated frame (page tables need zeroed memory).
9. Done - `paging.rs`: 3-level Sv39 tables, 512 × u64 per frame. `map(root, va, pa, flags)`.
10.Done - `paging.rs`: PTE flags V/R/W/X/A/D/G. Set A and D eagerly.
11.Done - `paging.rs`: identity map usable RAM RWX; UART and PLIC pages RW
12.Done - `paging.rs`: read `riscv,isa` from the CPU node. Set `menvcfg.PBMTE` when `svpbmt` is present, then map MMIO with PBMT=IO.
13.Done - `paging.rs`: `activate(root)` = `csrw satp, (root_addr >> 12) | (8 << 60)` then `sfence.vma zero, zero`.
14. Done -  `main.rs`: build tables in M-mode (`start()`), activate in `main()` after handover.
15. `kernel_trap.rs`: handle scause 12/13/15 (page faults). Print `scause`/`stval`, then park.
16. `utils.rs`: replace raw slice reads with `read_unaligned` before unaligned data reaches the FDT code.
17. Build and run after steps 4, 8, 14, 15. Marker bytes: `P` after paging active.
18. `alloc` crate: `build-std = ["core", "alloc"]` in `.cargo/config.toml`.
19. `heap.rs`: reserve 1 MiB from usable RAM; free-list allocator; `#[global_allocator]`; `#[alloc_error_handler]`. Zero dependencies.
20. Marker byte `H` after heap ready.
21. Done - `panic!` handler prints via UART.
22. `task.rs`: task table, `switch_to` asm, round-robin `schedule()`, boot context as task 0.
23. `paging.rs`: caller-owned page table entry flags, `new_task_root()`. `task.rs`: `spawn()` with user code, scratch and stack pages.
24. `kernel_trap.rs`: `_usertrapvec`, `TrapFrame` (31 registers, `sstatus`, `sepc`), return to user mode, environment call from user mode: write, exit. The kernel trap path does not change.
25. `task.rs`: `sched_yield`; two tasks, cooperative round robin.
26. Timer preemption of user tasks.
27. Address space identifier (ASID) per task, teardown of dead tasks, user page fault kills the task, not the kernel.
