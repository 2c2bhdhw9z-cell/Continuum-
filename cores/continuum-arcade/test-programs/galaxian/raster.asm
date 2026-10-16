; Galaxian board test: changes in the middle of a frame (GPL-3.0-only, cArcade's own).
; The board draws as the beam goes, so a change shows from the beam's next line on. After each
; NMI a delay (different every frame, sweeping the whole frame) leads to a column's scroll and
; another's colour changing, a tile code, a sprite moving, a shot moving, then the x flip, the y
; flip and the stars going on and off, each a little further down. The NMI puts it all back.

    DEFINE OUTNAME "raster.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld a, 5
    ld e, 3
    call fill1k
    ld hl, OBJ
    ld bc, 0x100
    xor a
    call fill
    xor a
    call colours
    ld hl, sprite_tab
    ld de, SPR
    ld bc, 16
    ldir
    ld hl, shot_tab
    ld de, SHOT
    ld bc, 8
    ldir
    ld a, 1
    ld (STARS), a
    jp run

sprite_tab:
    DB 100, 0x05, 0x01, 60
    DB 150, 0x46, 0x02, 120
    DB 60, 0x87, 0x03, 180
    DB 200, 0x08, 0x04, 30
shot_tab:
    DB 0, 120, 0, 100
    DB 0, 121, 0, 140

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

; In the NMI: everything back as it was, and this frame's delay.
frame:
    ld a, l
    ld (NOW), a
    xor a
    ld (FLIPX), a
    ld (FLIPY), a
    ld a, 1
    ld (STARS), a
    ld a, (SPR + 3)
    and 0x7F
    ld (SPR + 3), a
    ld a, 120
    ld (SHOT + 1), a
    ret

idle:
    ; the first delay: 64 + (frame * 37) & 1023, in units of 26 T-states
    ld a, (NOW)
    ld l, a
    ld h, 0
    ld d, h
    ld e, l
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, de
    add hl, hl
    add hl, hl
    add hl, de          ; HL = frame * 37
    ld a, h
    and 3
    ld h, a
    ld de, 64
    add hl, de
    ; odd frames reach the bottom half of the screen
    ld a, (NOW)
    rrca
    jr nc, .even
    ld de, 900
    add hl, de
.even:
    ld b, h
    ld c, l
    call delay
    ; a column's scroll
    ld a, (NOW)
    and 0x1F
    add a, a
    ld l, a
    ld h, OBJ >> 8
    ld a, (hl)
    add a, 40
    ld (hl), a
    ld bc, 30
    call delay
    ; another column's colour
    ld a, (NOW)
    add a, 5
    and 0x1F
    add a, a
    inc a
    ld l, a
    ld h, OBJ >> 8
    inc (hl)
    ld bc, 30
    call delay
    ; a tile code
    ld a, (NOW)
    ld l, a
    ld h, (VRAM >> 8) + 1
    inc (hl)
    ld bc, 25
    call delay
    ; a sprite moves right and back down
    ld a, (SPR + 3)
    xor 0x80
    ld (SPR + 3), a
    ld a, (SPR + 4)
    inc a
    ld (SPR + 4), a
    ld bc, 25
    call delay
    ; a shot moves
    ld a, (SHOT + 1)
    add a, 9
    ld (SHOT + 1), a
    ld bc, 40
    call delay
    ; the flips and the stars, on then off
    ld a, 1
    ld (FLIPX), a
    ld bc, 40
    call delay
    ld a, 1
    ld (FLIPY), a
    ld bc, 40
    call delay
    xor a
    ld (FLIPX), a
    ld (STARS), a
    ld bc, 40
    call delay
    xor a
    ld (FLIPY), a
    ld a, 1
    ld (STARS), a
    ret

NOW     equ USER

    include "end.inc"
