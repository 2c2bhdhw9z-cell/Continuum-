; Pac-Man board test: bootleg boards with ROM at 8000h (GPL-3.0-only, cArcade's own).
; Many Ms. Pac-Man bootlegs wire the Z80's A15, so 8000h-BFFFh is a second set of ROMs instead of
; a mirror of 0000h-3FFFh. This program's set-up and per-frame work run from 8000h, and each frame
; it shows bytes read from both halves of the map, which differ.

    DEFINE OUTNAME "upper.bin"
    include "hw.inc"
    include "start.inc"

    org 0x0123
    db 0x01, 0x23
    org 0x1234
    db 0x12, 0x34

    org 0x8000
init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x52
    ldir
    ld hl, CRAM
    ld de, CRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x0D
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jp run

frame:
    ld a, l
    ld (VRAM + 0x3C4), a
    ld hl, VRAM + 0x040
    ld a, (0x0123)
    ld (hl), a
    inc hl
    ld a, (0x8123)
    ld (hl), a
    inc hl
    ld a, (0x1234)
    ld (hl), a
    inc hl
    ld a, (0x9234)
    ld (hl), a
    inc hl
    ld a, (0x0124)
    ld (hl), a
    inc hl
    ld a, (0x8124)
    ld (hl), a
    ret

idle:
    ret

    org 0x8123
    db 0x81, 0x24
    org 0x9234
    db 0x92, 0x35

    SAVEBIN "upper-hi.bin", 0x8000, 0x2000
    include "end.inc"
