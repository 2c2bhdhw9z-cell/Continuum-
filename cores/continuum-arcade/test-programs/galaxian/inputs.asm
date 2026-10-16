; Galaxian board test: the inputs (GPL-3.0-only, cArcade's own).
; Every frame each bit of IN0, IN1 and IN2 shows as a tile (blank for 0), and the three bytes as
; tile codes too; the start buttons drive the lamps and the coins the coin counter and lockout,
; as a game does (none of those shows on screen).

    DEFINE OUTNAME "inputs.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, OBJ
    ld bc, 0x100
    xor a
    call fill
    ld a, 1
    call colours
    jp run

frame:
    ld de, VRAM + 0x42
    ld a, (IN0)
    call bits
    ld de, VRAM + 0x82
    ld a, (IN1)
    call bits
    ld de, VRAM + 0xC2
    ld a, (IN2)
    call bits
    ld a, (IN0)
    ld (VRAM + 0x104), a
    ld a, (IN1)
    ld (VRAM + 0x106), a
    ld a, (IN2)
    ld (VRAM + 0x108), a
    ld a, (IN1)
    ld (LAMP1), a
    rrca
    ld (LAMP2), a
    ld a, (IN0)
    ld (COUNTER), a
    ld (LOCKOUT), a
    ret

; Eight tiles at DE, two apart, bit 7 first: tile 4 for a 1, blank for a 0.
bits:
    ld b, 8
.b: rlca
    ld c, a
    ld a, 0
    jr nc, .z
    ld a, 4
.z: ld (de), a
    inc de
    inc de
    ld a, c
    djnz .b
    ret

idle:
    ret

    include "end.inc"
