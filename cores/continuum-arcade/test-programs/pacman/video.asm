; Pac-Man board test: the tile layer (GPL-3.0-only, cArcade's own).
; Fills all 1024 tile codes and colours (every code, every colour, the junk top bits of the
; colour too), then each frame changes a band of 32 of them, so over 32 frames every byte of
; video and colour RAM changes. Along the way: the screen flip (on at frame 50, off at 80, then
; every 7 frames), writes and reads through the mirrors of video RAM at 6000h, C000h and E000h,
; and the empty 4800h-4BFFh area (writes go nowhere, reads give BFh), shown as tiles.

    DEFINE OUTNAME "video.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld a, 3
    ld e, 7
    call fill1k
    ld hl, CRAM
    ld a, 0x11
    ld e, 13
    call fill1k
    ; sprites: all off the screen
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jp run

; 1 KB at HL from A, adding E each byte.
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
    ; the band: 32 codes and 32 colours at (frame * 32) & 3FFh
    ld a, l
    and 0x1F
    ld e, a
    ld d, 0
    ex de, hl
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    ex de, hl               ; DE = band offset
    push hl
    ld hl, VRAM
    add hl, de
    ld b, 32
.v: inc (hl)
    inc hl
    djnz .v
    ld hl, CRAM
    add hl, de
    ld b, 32
.c: ld a, (hl)
    add a, 5
    ld (hl), a
    inc hl
    djnz .c
    pop hl
    ; the flip: on at 50, off at 80, then flips every 7 frames
    ld a, h
    or a
    jr nz, .late
    ld a, l
    cp 50
    jr z, .on
    cp 80
    jr z, .off
    jr c, .mirrors
.late:
    push hl
    ld de, 7
    xor a
.mod: sbc hl, de
    jr nc, .mod
    add hl, de
    ld a, l
    pop hl
    or a
    jr nz, .mirrors
    ld a, (FLIP_COPY)
    xor 1
    jr .set
.on: ld a, 1
    jr .set
.off: xor a
.set: ld (FLIP), a
    ld (FLIP_COPY), a
.mirrors:
    ; writes through the mirrors land in video RAM
    ld a, l
    ld (VRAM + 0x2000 + 0x123), a
    cpl
    ld (VRAM + 0x8000 + 0x234), a
    rlca
    ld (CRAM + 0xA000 + 0x345), a
    ; reads through them too: copy three bytes to the score rows
    ld a, (VRAM + 0xA000 + 0x0C0)
    ld (VRAM + 0x3C5), a
    ld a, (CRAM + 0x2000 + 0x0C1)
    ld (VRAM + 0x3C6), a
    ld a, (VRAM + 0x8000 + 0x0C2)
    ld (VRAM + 0x3C7), a
    ; 4800h-4BFFh: writes are lost, reads are BFh
    ld (0x4800 + 0x55), a
    ld a, (0x4800 + 0x55)
    ld (VRAM + 0x3C8), a
    ld a, (0x6800 + 0x3FF)
    ld (VRAM + 0x3C9), a
    ret

idle:
    ret

FLIP_COPY equ USER

    include "end.inc"
