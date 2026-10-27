; Pac-Man board test: the graphics wiring on a board whose program ROMs are only at 0000h-0FFFh
; and 8000h-BFFFh (Woodpecker's) (GPL-3.0-only, cArcade's own). Like wiring.asm, but all its code
; is in the first 4 KB and its interrupt is mode 1 (RST 38h), so it needs nothing at 1000h-3FFFh:
; every tile code and colour on the screen, then the eight sprites stepping through all 64
; sprite codes, both flips and every colour, and bytes from 8000h-BFFFh in the score rows.

    DEVICE NOSLOT64K
    include "hw.inc"

    org 0
    di
    ld sp, STACK
    jp init

    org 0x38
    push af
    push bc
    push de
    push hl
    push ix
    push iy
    xor a
    ld (IRQEN), a
    ld (WDOG), a
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    call frame
    ld a, 1
    ld (IRQEN), a
    pop iy
    pop ix
    pop hl
    pop de
    pop bc
    pop af
    ei
    reti

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld bc, 0x400
.v: ld a, l
    xor h
    add a, c
    ld (hl), a
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .v
    ld hl, CRAM
    ld bc, 0x400
.c: ld a, l
    rrca
    rrca
    xor c
    ld (hl), a
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .c
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    im 1
    ld a, 1
    ld (IRQEN), a
    ei
.loop:
    halt
    jr .loop

frame:
    ld ix, SPRATTR
    ld iy, SPRXY
    ld b, 0
.spr:
    ld a, b
    add a, a
    add a, a
    add a, a
    add a, b
    add a, l
    and 0x3F
    add a, a
    add a, a
    ld c, a
    ld a, l
    rrca
    rrca
    rrca
    and 3
    or c
    ld (ix + 0), a
    ld a, l
    rrca
    rrca
    add a, b
    add a, b
    add a, b
    and 0x1F
    ld (ix + 1), a
    ld a, b
    rlca
    rlca
    rlca
    rlca
    rlca
    add a, l
    add a, 24
    ld (iy + 0), a
    ld a, l
    add a, a
    add a, b
    add a, 40
    ld (iy + 1), a
    inc ix
    inc ix
    inc iy
    inc iy
    inc b
    ld a, b
    cp 8
    jr nz, .spr
    ld h, 0x80
    ld a, (hl)
    ld (VRAM + 0x3C2), a
    ld h, 0xA7
    ld a, (hl)
    ld (VRAM + 0x3C3), a
    ld a, l
    cp 60
    ld a, 0
    jr c, .f
    inc a
.f: ld (FLIP), a
    ret

    org 0x8000
c = 0
    DUP 0x4000
    db (c * 29 + 3 + (c >> 7)) & 255
c = c + 1
    EDUP

    SAVEBIN "lowgfx.bin", 0, 0x4000
    SAVEBIN "lowgfx-hi.bin", 0x8000, 0x4000
