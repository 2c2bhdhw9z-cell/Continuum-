; Donkey Kong board test: the sprites (GPL-3.0-only, cArcade's own).
; The 8257 copies a list of 96 sprites to the sprites' RAM every frame. 24 of them move at their
; own speeds through every position and off every edge (a sprite leaving one side shows on the
; other), their code byte (both flips, the code's top bit) and colour byte (flip x, bank bit,
; colour, junk) moving on; 20 more sit on the same lines, more than the 16 a line can show. The
; tile layer shows a few rows of tiles behind them. The palette bank steps every 32 frames, the
; screen flips at 100 and back at 170, and from 200 to 240 the sprite bank shows the second 128
; (written straight into 7200h-73FFh).

    DEFINE OUTNAME "sprites.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ; Rows 10-13 of tiles, behind the sprites.
    ld hl, VRAM + 10 * 32
    ld b, 128
    ld a, 0x41
.t: ld (hl), a
    add a, 7
    inc hl
    djnz .t
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRLIST
    ld bc, 0x180
    xor a
    call fill
    ; The movers.
    ld hl, start_tab
    ld de, SPRLIST
    ld bc, 24 * 4
    ldir
    ; The crowd: 20 sprites at y 90-97, x 0, 12, 24 ...
    ld ix, SPRLIST + 24 * 4
    ld b, 20
    ld c, 0
    ld e, 90
.c: ld (ix + 0), e
    ld a, c
    add a, 0x21
    ld (ix + 1), a
    ld a, c
    and 0x0F
    ld (ix + 2), a
    ld (ix + 3), c
    ld a, c
    add a, 12
    ld c, a
    ld a, e
    xor 3
    ld e, a
    inc ix
    inc ix
    inc ix
    inc ix
    djnz .c
    ; The second bank: 128 sprites straight into 7200h, a grid.
    ld ix, SPRRAM + 0x200
    ld b, 128
    ld c, 0
.g: ld a, c
    and 0xF0
    add a, 20
    ld (ix + 0), a
    ld a, c
    ld (ix + 1), a
    rrca
    rrca
    ld (ix + 2), a
    ld a, c
    rlca
    rlca
    rlca
    rlca
    and 0xF0
    add a, 6
    ld (ix + 3), a
    inc c
    inc ix
    inc ix
    inc ix
    inc ix
    djnz .g
    ld a, 1
    ld (DMAON), a
    jp run

; Each mover's y, code byte, colour byte and x at the start...
start_tab:
    DB 20, 0x01, 0x00, 7
    DB 40, 0x49, 0x09, 37
    DB 60, 0x8A, 0x12, 67
    DB 80, 0xC3, 0x1B, 97
    DB 100, 0x14, 0x24, 127
    DB 120, 0x5D, 0x2D, 157
    DB 140, 0x9E, 0x36, 187
    DB 160, 0xEF, 0x3F, 217
    DB 180, 0x30, 0x48, 247
    DB 200, 0x71, 0x51, 250
    DB 220, 0xB2, 0x5A, 3
    DB 240, 0xF3, 0x63, 12
    DB 250, 0x04, 0x6C, 100
    DB 5, 0x45, 0x75, 200
    DB 30, 0x86, 0x7E, 1
    DB 50, 0xC7, 0x87, 255
    DB 70, 0x28, 0x90, 128
    DB 110, 0x69, 0x99, 64
    DB 130, 0xAA, 0xA2, 32
    DB 150, 0xEB, 0xAB, 16
    DB 170, 0x1C, 0xB4, 240
    DB 190, 0x5D, 0xBD, 230
    DB 210, 0x9E, 0xC6, 8
    DB 230, 0xDF, 0xCF, 248
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
    DB 0, 1, 23
    DB 1, 0, 29
    DB 0xFF, 0xFF, 31
    DB 4, 0xFC, 37
    DB 0xFC, 4, 41
    DB 2, 2, 43
    DB 0xFE, 1, 47
    DB 1, 0xFE, 53
    DB 7, 5, 59
    DB 0xF9, 3, 61
    DB 3, 0xF9, 67
    DB 0, 0xFF, 71
    DB 0xFF, 0, 73
    DB 6, 6, 79
    DB 0xFA, 0xFA, 83
    DB 1, 3, 89

frame:
    push hl
    ld ix, SPRLIST
    ld iy, move_tab
    ld b, 24
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
    ld de, 3
    add iy, de
    djnz .s
    pop hl
    ; The palette bank, every 32 frames.
    ld a, l
    and 31
    jr nz, .nopal
    ld a, l
    rrca
    rrca
    rrca
    rrca
    rrca
    ld (PALBANK), a
    rrca
    ld (PALBANK + 1), a
.nopal:
    ld a, h
    or a
    ret nz
    ld a, l
    cp 100
    jr z, .flip
    cp 170
    jr z, .unflip
    cp 200
    jr z, .bank1
    cp 240
    ret nz
    xor a
    ld (SPRBANK), a
    ret
.bank1:
    ld a, 1
    ld (SPRBANK), a
    ret
.flip:
    ld a, 1
    ld (FLIP), a
    ret
.unflip:
    xor a
    ld (FLIP), a
    ret

idle:
    ret

    include "end.inc"
