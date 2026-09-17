/*
 * Minimal GBA cartridge layout.
 *
 * The first four bytes of a GBA ROM must be an ARM branch to the entry point — its
 * opcode byte lands at offset 3 as 0xEA, which is exactly what mgba checks. `.init`
 * is therefore placed first and holds nothing but that branch.
 *
 * There is deliberately no `.data` copy loop: the ROM avoids initialised globals, so
 * writable data never needs relocating from ROM into RAM. `.bss` lives in IWRAM and is
 * zeroed by the entry stub.
 */

ENTRY(_start)

MEMORY
{
  ewram (rwx) : ORIGIN = 0x02000000, LENGTH = 256K
  iwram (rwx) : ORIGIN = 0x03000000, LENGTH = 32K
  rom   (rx)  : ORIGIN = 0x08000000, LENGTH = 32M
}

SECTIONS
{
  .init : { KEEP(*(.init)) } > rom
  .text : { *(.text*) } > rom
  .rodata : { *(.rodata*) } > rom

  /* Anything writable would need a ROM→RAM copy at boot; the linker errors below make
     that impossible to introduce accidentally. */
  .bss (NOLOAD) :
  {
    __bss_start = .;
    *(.bss*)
    *(COMMON)
    __bss_end = .;
  } > iwram

  /DISCARD/ : { *(.comment) *(.ARM.attributes) *(.note*) }
}

ASSERT(SIZEOF(.data) == 0, "this ROM must not have initialised writable data")
