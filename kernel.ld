OUTPUT_ARCH("riscv")
ENTRY(_entry)

SECTIONS
{
  . = 0x80000000;            /* qemu -kernel jumps here */

  .text : {
    *(.text.entry)           /* _entry first */
    *(.text .text.*)
    . = ALIGN(4K);
    PROVIDE(etext = .);
  }

  .rodata : {
    . = ALIGN(16);
    *(.srodata .srodata.*)
    . = ALIGN(16);
    *(.rodata .rodata.*)
  }

  .data : {
    . = ALIGN(16);
    *(.sdata .sdata.*)
    . = ALIGN(16);
    *(.data .data.*)
  }

  .bss : {
    . = ALIGN(16);
    *(.sbss .sbss.*)
    . = ALIGN(16);
    *(.bss .bss.* COMMON)
    . = ALIGN(16);
  }

  PROVIDE(end = .);          /* kalloc will need this later */
}
