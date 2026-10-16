; Galaxian board test: the sprites (GPL-3.0-only, cArcade's own).
; Eight sprites move at their own speeds, both ways, through all 256 positions and off every edge;
; every 4 frames each one's code byte moves on (through every code and both flips), every 8 its
; colour byte (junk top bits too), so they overlap in every order. The screen flips: x at 100, y at
; 150, both off at 200, then x every 11 frames and y every 17. The tile layer is blank (tile 0).

    DEFINE OUTNAME "sprites.bin"
    include "hw.inc"
    include "start.inc"

FXC     equ USER
FYC     equ USER + 1
FXV     equ USER + 2
FYV     equ USER + 3

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
    ld de, SPR
    ld bc, 32
    ldir
    ld a, 11
    ld (FXC), a
    ld a, 17
    ld (FYC), a
    jp run

; Each sprite's y, code byte, colour byte and x at the start...
start_tab:
    DB 20, 0x01, 0x00, 7
    DB 40, 0x49, 0x09, 37
    DB 60, 0x8A, 0x12, 67
    DB 80, 0xC3, 0x1B, 97
    DB 100, 0x14, 0x24, 127
    DB 120, 0x5D, 0x2D, 157
    DB 140, 0x9E, 0x36, 187
    DB 160, 0xEF, 0x3F, 217
; ...and how far its y and x move each frame, and its code byte every 4 frames.
move_tab:
    DB 1, 1, 1
    DB 0xFF, 2, 3
    DB 2, 0xFF, 5
    DB 3, 3, 7
    DB 0xFE, 0xFD, 11
    DB 1, 0xFE, 13
    DB 0xFD, 1, 17
    DB 5, 7, 19

frame:
    push hl
    ld ix, SPR
    ld iy, move_tab
    ld b, 8
.s:
    ld a, (ix + 0)
    add a, (iy + 0)
    ld (ix + 0), a
    ld a, (ix + 3)
    add a, (iy + 1)
    ld (ix + 3), a
    ld a, l
    and 3
    jr nz, .nocode
    ld a, (ix + 1)
    add a, (iy + 2)
    ld (ix + 1), a
.nocode:
    ld a, l
    and 7
    jr nz, .nocolour
    ld a, (ix + 2)
    add a, 0x13
    ld (ix + 2), a
.nocolour:
    ld de, 4
    add ix, de
    inc iy
    inc iy
    inc iy
    djnz .s
    pop hl
    ; the flips
    ld a, h
    or a
    jr nz, .late
    ld a, l
    cp 100
    jr z, .xon
    cp 150
    jr z, .yon
    cp 200
    jr z, .off
    cp 210
    ret c
.late:
    ld hl, FXC
    dec (hl)
    jr nz, .ny
    ld (hl), 11
    ld a, (FXV)
    xor 1
    ld (FXV), a
    ld (FLIPX), a
.ny:
    ld hl, FYC
    dec (hl)
    ret nz
    ld (hl), 17
    ld a, (FYV)
    xor 1
    ld (FYV), a
    ld (FLIPY), a
    ret
.xon:
    ld a, 1
    ld (FXV), a
    ld (FLIPX), a
    ret
.yon:
    ld a, 1
    ld (FYV), a
    ld (FLIPY), a
    ret
.off:
    xor a
    ld (FXV), a
    ld (FLIPX), a
    ld (FYV), a
    ld (FLIPY), a
    ret

idle:
    ret

    include "end.inc"
