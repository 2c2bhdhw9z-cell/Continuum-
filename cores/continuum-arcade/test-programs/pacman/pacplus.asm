; Pac-Man board test: Pac-Man Plus's program decoder (GPL-3.0-only, cArcade's own).
; Assembled plain; the test scrambles it the way Pac-Man Plus's ROMs are scrambled, so the board's
; decoder has to unscramble every byte (instructions and data alike) for it to run at all. Each
; frame it adds up one 256-byte block of its own program as data, shows the sum, and moves a
; sprite; the tile layer is filled as in video.asm.

    DEFINE OUTNAME "pacplus.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld a, 9
    ld e, 11
    call fill1k
    ld hl, CRAM
    ld a, 0x15
    ld e, 7
    call fill1k
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ld a, 0x1D
    ld (SPRATTR), a
    ld a, 0x0C
    ld (SPRATTR + 1), a
    jp run

fill1k:
    ld c, 4
.o: ld b, 0
.i: ld (hl), a
    add a, e
    inc hl
    djnz .i
    dec c
    jr nz, .o
    ret

frame:
    ; the sum of block (frame & 3Fh) of the program
    ld a, l
    and 0x3F
    ld h, a
    ld l, 0
    ld de, 0
    ld b, 0
.sum:
    ld a, (hl)
    add a, e
    ld e, a
    jr nc, .nc
    inc d
.nc:
    inc hl
    djnz .sum
    ld hl, (FRAMES)
    ld a, l
    and 0x1F
    ld c, a
    ld b, 0
    ld hl, VRAM + 0x040
    add hl, bc
    add hl, bc
    ld (hl), e
    inc hl
    ld (hl), d
    ; sprite 0 moves diagonally
    ld hl, (FRAMES)
    ld a, l
    add a, a
    ld (SPRXY), a
    ld a, l
    add a, 0x40
    ld (SPRXY + 1), a
    ret

idle:
    ret

    include "end.inc"
