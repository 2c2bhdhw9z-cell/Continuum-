/* Linker script for building mgba-emu/suite with a stock arm-none-eabi-gcc.
   ROM is 32M max (the emulator sizes it from the file); EWRAM 256K, IWRAM 32K
   with the top 0x600 bytes reserved for the mode stacks (the BIOS workspace
   0x03007F00-0x03007FFF sits above them). */

MEMORY {
    rom   : ORIGIN = 0x08000000, LENGTH = 32M
    ewram : ORIGIN = 0x02000000, LENGTH = 256K
    iwram : ORIGIN = 0x03000000, LENGTH = 0x7A00
}

__sp_irq = 0x03007F00;
__sp_fiq = 0x03007E00;
__sp_svc = 0x03007D00;
__sp_abt = 0x03007C00;
__sp_und = 0x03007B00;
__sp_usr = 0x03007A00;

SECTIONS {
    .text 0x08000000 : {
        __text_start = .;
        KEEP(*(.init))
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.glue_7t) *(.glue_7) *(.vfp11_veneer) *(.v4_bx)
        *(.ARM.exidx .ARM.exidx.*)
        *(.eh_frame .eh_frame.*)
        __text_end = .;
    } >rom

    .iwram : AT(ADDR(.text) + SIZEOF(.text)) {
        __iwram_start = .;
        *(.iwram .iwram.*)
        __iwram_end = .;
    } >iwram
    __iwram_lma = LOADADDR(.iwram);

    .ewram : AT(LOADADDR(.iwram) + SIZEOF(.iwram)) {
        __ewram_start = .;
        *(.ewram .ewram.*)
        __ewram_end = .;
    } >ewram
    __ewram_lma = LOADADDR(.ewram);

    .data : AT(LOADADDR(.ewram) + SIZEOF(.ewram)) {
        __data_start = .;
        *(.data .data.*)
        __data_end = .;
    } >ewram
    __data_lma = LOADADDR(.data);

    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } >ewram
    end = __bss_end;

    /DISCARD/ : {
        *(.comment)
        *(.note .note.*)
        *(.debug*)
    }
}
