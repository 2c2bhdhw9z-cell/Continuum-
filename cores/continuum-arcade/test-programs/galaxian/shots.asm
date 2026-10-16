; Galaxian board test: the shots (GPL-3.0-only, cArcade's own).
; The eight shot entries (seven shells and the missile) move up and down and across at their own
; speeds, through all 256 lines and positions; pairs of them share lines now and then, so the rule
; that the last entry matching a line wins shows. The screen flips y at 60 and back at 120, x at 90
; and back at 150, then both every 23 frames. No tiles or sprites show.

    DEFINE OUTNAME "shots.bin"
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
    ld hl, start_tab
    ld de, SHOT
    ld bc, 32
    ldir
    jp run

; Each entry: (unused), y, (unused), x.
start_tab:
    DB 0x11, 30, 0x22, 10
    DB 0x33, 31, 0x44, 50
    DB 0x55, 60, 0x66, 90
    DB 0x77, 60, 0x88, 130
    DB 0x99, 100, 0xAA, 170
    DB 0xBB, 140, 0xCC, 210
    DB 0xDD, 140, 0xEE, 250
    DB 0xFF, 140, 0x01, 128
; How far each moves each frame: y, x.
move_tab:
    DB 1, 3
    DB 1, 0xFD
    DB 0xFF, 1
    DB 2, 2
    DB 0xFE, 5
    DB 3, 0xFF
    DB 1, 7
    DB 0xFF, 0xF9

frame:
    push hl
    ld ix, SHOT
    ld iy, move_tab
    ld b, 8
.s:
    ld a, (ix + 1)
    add a, (iy + 0)
    ld (ix + 1), a
    ld a, (ix + 3)
    add a, (iy + 1)
    ld (ix + 3), a
    ld de, 4
    add ix, de
    inc iy
    inc iy
    djnz .s
    pop hl
    ld a, h
    or a
    jr nz, .late
    ld a, l
    cp 60
    jr z, .yon
    cp 90
    jr z, .xon
    cp 120
    jr z, .yoff
    cp 150
    jr z, .xoff
    ret c
.late:
    ; then both flip every 23 frames
    ld a, l
    sub 150
.m: sub 23
    jr nc, .m
    add a, 23
    ret nz
    ld a, (FLIPV)
    xor 1
    ld (FLIPV), a
    ld (FLIPX), a
    ld (FLIPY), a
    ret
.yon:
    ld a, 1
    ld (FLIPY), a
    ret
.yoff:
    xor a
    ld (FLIPY), a
    ret
.xon:
    ld a, 1
    ld (FLIPX), a
    ret
.xoff:
    xor a
    ld (FLIPX), a
    ret

FLIPV   equ USER

idle:
    ret

    include "end.inc"
