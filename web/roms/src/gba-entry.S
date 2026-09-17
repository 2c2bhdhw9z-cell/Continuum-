@ GBA entry stub.
@
@ Four things have to happen before C can run, and none of them can be expressed in C:
@ the first ROM word must be a branch (its 0xEA opcode byte at offset 3 is what mgba
@ checks to recognise a GBA ROM), the stack pointer must be set, .bss must be zeroed,
@ and the header bytes between the branch and 0xC0 must exist as padding.
@
@ The 156 bytes at 0x04 are the Nintendo logo on a commercial cartridge. They are left
@ as zero here: mgba does not check them, and copying Nintendo's logo data into this
@ repository would make the ROM not ours to distribute.

    .section .init, "ax"
    .arm
    .global _start
    .align 2

_start:
    b   boot                        @ assembles to 0xEA......, so byte 3 = 0xEA

    @ 0x04..0xBF: header. Zero-filled; make-gba-rom.sh patches the title, the fixed
    @ 0x96 at 0xB2 and the header checksum at 0xBD.
    .space 0xBC, 0

boot:
    @ Interrupts are never enabled, so only the user-mode stack matters.
    @ 0x03007F00 is where the BIOS leaves SP on real hardware.
    mov     sp, #0x03000000
    add     sp, sp, #0x7F00

    @ Zero .bss: the C code assumes it, and IWRAM is not cleared on boot.
    ldr     r0, =__bss_start
    ldr     r1, =__bss_end
    mov     r2, #0
bss_loop:
    cmp     r0, r1
    strlo   r2, [r0], #4
    blo     bss_loop

    bl      main

    @ main never returns; if it somehow does, stop rather than execute ROM padding.
hang:
    b       hang
