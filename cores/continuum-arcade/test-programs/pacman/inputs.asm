; Pac-Man board test: the inputs and DIP switches (GPL-3.0-only, cArcade's own).
; Each frame it reads the four ports (5000h, 5040h, 5080h, 50C0h) and copies of them at their
; mirrors (the board ignores A0-A5, A8-A11, A13 and A15 there), and shows each byte twice: as one
; tile whose code is the byte, and as eight tiles, one per bit. The test presses buttons, moves
; the sticks and sets DIP switches frame by frame, the same way on cArcade and on MAME.

    DEFINE OUTNAME "inputs.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x20
    ldir
    ld hl, CRAM
    ld de, CRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x05
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jp run

; Shows A at HL: one tile coded A, then eight tiles for its bits (code 81h set, 7Eh clear), down
; the column; HL moves on nine.
show:
    ld (hl), a
    inc hl
    ld b, 8
.b: rlca
    ld c, 0x81
    jr c, .one
    ld c, 0x7E
.one:
    ld (hl), c
    inc hl
    djnz .b
    ret

frame:
    ld hl, VRAM + 0x040     ; playfield column 27 (the right edge), top down
    ld a, (IN0)
    call show
    ld a, (IN1)
    call show
    ld a, (DSW1)
    call show
    ld hl, VRAM + 0x060     ; column 26
    ld a, (DSW2)
    call show
    ld a, (0x503F)          ; IN0, A0-A5 set
    call show
    ld a, (0x5F7F)          ; IN1, A8-A11 too
    call show
    ld hl, VRAM + 0x080     ; column 25
    ld a, (0x70BF)          ; DSW1 at A13
    call show
    ld a, (0xD0C0)          ; DSW2 at A15
    call show
    ld a, (0xF000)          ; IN0 at A13 and A15
    call show
    ret

idle:
    ret

    include "end.inc"
