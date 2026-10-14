; Pac-Man board test: the eight sprites (GPL-3.0-only, cArcade's own).
; Over a busy tile background, eight sprites move at different speeds in both directions, so
; over a few hundred frames each passes every position: across each other (sprite 0 on top),
; off the edges and back in at the other side, and under the two status rows at the top and
; bottom of the screen, where the board doesn't draw them. Every 16 frames each one's code,
; flips and colour change; colours whose pens look up colour 0 show the background through.

    DEFINE OUTNAME "sprites.bin"
    include "hw.inc"
    include "start.inc"

POSA equ USER           ; 8 bytes: what goes to 5060h + 2n
POSB equ USER + 8       ; 8 bytes: what goes to 5061h + 2n

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld a, 0x40
    ld e, 1
    call fill1k
    ld hl, CRAM
    ld a, 0x02
    ld e, 3
    call fill1k
    ld hl, start_pos
    ld de, POSA
    ld bc, 16
    ldir
    ld hl, start_attr
    ld de, SPRATTR
    ld bc, 16
    ldir
    call place
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

; Writes the positions out to 5060h-506Fh.
place:
    ld ix, POSA
    ld hl, SPRXY
    ld b, 8
.p: ld a, (ix + 0)
    ld (hl), a
    inc hl
    ld a, (ix + 8)
    ld (hl), a
    inc hl
    inc ix
    djnz .p
    ret

frame:
    push hl
    ld ix, POSA
    ld iy, speeds
    ld b, 8
.m: ld a, (ix + 0)
    add a, (iy + 0)
    ld (ix + 0), a
    ld a, (ix + 8)
    add a, (iy + 8)
    ld (ix + 8), a
    inc ix
    inc iy
    djnz .m
    call place
    pop hl
    ld a, l
    and 15
    ret nz
    ; every 16 frames: the next code (and flips), and colour + 3
    ld hl, SPRATTR
    ld b, 8
.a: ld a, (hl)
    add a, 5
    ld (hl), a
    inc hl
    ld a, (hl)
    add a, 3
    ld (hl), a
    inc hl
    djnz .a
    ret

idle:
    ret

start_pos:
    db 0x10, 0x14, 0x50, 0x70, 0x90, 0xB0, 0xD0, 0xF0
    db 0x20, 0x24, 0xA0, 0xE0, 0x00, 0x40, 0x80, 0xC0
start_attr:
    db 0x00, 0x01, 0x06, 0x0B, 0x10, 0x07, 0x12, 0x09
    db 0x17, 0xFE, 0x1A, 0x1C, 0x13, 0x08, 0x0F, 0x16
; per sprite: the change to POSA, then to POSB, each frame
speeds:
    db 1, 0xFF, 2, 3, 0xFE, 5, 7, 0xFD
    db 2, 1, 0xFF, 0xFE, 3, 1, 4, 0xFB

    include "end.inc"
