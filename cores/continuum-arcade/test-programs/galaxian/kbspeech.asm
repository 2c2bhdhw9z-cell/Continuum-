; King & Balloon's speech board test program (GPL-3.0-only, cArcade's own): a Z80 at 2.5 MHz with
; no interrupts. It reads the command latch (I/O port 0) and plays through the 4-bit DAC (any
; port's write, the top four bits): command 0 a slow square wave, 1 a ramp, 2 a fast square wave,
; 3 a pattern of levels from its ROM. The command is read again between steps, so a new one starts
; at once.

    DEVICE NOSLOT64K
    org 0
    di
    ld sp, 0x0400
.loop:
    in a, (0)
    and 3
    jr z, .slow
    dec a
    jr z, .ramp
    dec a
    jr z, .fast
    ; 3: levels from the table, one per 40 T-states or so
    ld hl, table
    ld b, 32
.t: ld a, (hl)
    out (0), a
    inc hl
    nop
    nop
    djnz .t
    jr .loop
.slow:
    ld a, 0xF0
    out (0), a
    ld b, 60
.s1: djnz .s1
    xor a
    out (0), a
    ld b, 60
.s2: djnz .s2
    jr .loop
.ramp:
    xor a
    ld b, 16
.r: out (0), a
    add a, 0x10
    djnz .r
    jr .loop
.fast:
    ld a, 0xA0
    out (0), a
    ld a, 0x30
    out (0), a
    jr .loop

table:
    db 0x00, 0x30, 0x70, 0xB0, 0xF0, 0xD0, 0x90, 0x50, 0x10, 0x20, 0x60, 0xE0, 0xC0, 0x80, 0x40, 0x00
    db 0xF0, 0x00, 0xF0, 0x00, 0x80, 0x80, 0x40, 0xC0, 0x20, 0xA0, 0x60, 0xE0, 0x10, 0x90, 0x50, 0xD0

    SAVEBIN "kbspeech.bin", 0, 0x2000
