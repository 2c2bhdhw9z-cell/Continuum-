; Galaxian board test: the star field (GPL-3.0-only, cArcade's own).
; The stars come on at frame 2 (in VBLANK) and drift; at 50 they go off, and at 52 come on again
; mid-frame (their pattern starts where the beam is then). The screen flips x at 90 (the stars
; drift the other way) and back at 130; flips y at 150 (the stars don't care). At 170 they go off
; and on again mid-frame lower down; at 200 the x flip goes on mid-frame, and off at 215; at 230
; they go off mid-frame and stay off, and at 240 come back on. No tiles, sprites or shots show.

    DEFINE OUTNAME "stars.bin"
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
    jp run

frame:
    ld (NOW), hl
    ld a, h
    or a
    ret nz
    ld a, l
    cp 2
    jr z, .on
    cp 50
    jr z, .off
    cp 90
    jr z, .xon
    cp 130
    jr z, .xoff
    cp 150
    jr z, .yon
    cp 240
    jr z, .on
    ret
.on:
    ld a, 1
    ld (STARS), a
    ret
.off:
    xor a
    ld (STARS), a
    ret
.xon:
    ld a, 1
    ld (FLIPX), a
    ret
.xoff:
    xor a
    ld (FLIPX), a
    ret
.yon:
    ld a, 1
    ld (FLIPY), a
    ret

; After the NMI: the mid-frame changes, a delay into the frame.
idle:
    ld hl, (NOW)
    ld a, h
    or a
    ret nz
    ld a, l
    cp 52
    jr z, .on52
    cp 170
    jr z, .offon
    cp 200
    jr z, .xon
    cp 215
    jr z, .xoff
    cp 230
    jr z, .off
    ret
.on52:
    ld bc, 600
    call delay
    ld a, 1
    ld (STARS), a
    ret
.offon:
    ld bc, 900
    call delay
    xor a
    ld (STARS), a
    ld bc, 300
    call delay
    ld a, 1
    ld (STARS), a
    ret
.xon:
    ld bc, 1200
    call delay
    ld a, 1
    ld (FLIPX), a
    ret
.xoff:
    ld bc, 500
    call delay
    xor a
    ld (FLIPX), a
    ret
.off:
    ld bc, 1500
    call delay
    xor a
    ld (STARS), a
    ret

NOW     equ USER

    include "end.inc"
