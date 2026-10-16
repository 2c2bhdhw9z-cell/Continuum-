; Donkey Kong board test: the inputs (GPL-3.0-only, cArcade's own).
; Each frame the program reads IN0, IN1, IN2 and the DIP switches and shows their bits as tiles,
; one row each, and the frame count in hex; the test presses every control and sets every switch
; in turn.

    DEFINE OUTNAME "inputs.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    jp run

frame:
    ld hl, VRAM + 8 * 32 + 12
    ld a, (IN0)
    call bits
    ld a, (IN1)
    call bits
    ld a, (IN2)
    call bits
    ld a, (DSW0)
    call bits
    ld a, (FRAMES)
    call hex
    ret

idle:
    ret

    include "end.inc"
