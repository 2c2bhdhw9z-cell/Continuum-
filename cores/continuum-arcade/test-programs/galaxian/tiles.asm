; Galaxian board test: the tile layer (GPL-3.0-only, cArcade's own).
; Fills video RAM with every tile code and gives the 32 columns every colour (with junk in the
; colour bytes' top bits), then each frame scrolls each column by its own step, changes a band of
; 32 tile codes, and every 4 frames moves the colours on. The screen flips: x at 40, y at 70, x
; off at 100, y off at 130, then x every 9 frames and y every 13. Writes and reads go through the
; mirrors of video, object and work RAM, and reads of nothing and of the input ports show as tiles.

    DEFINE OUTNAME "tiles.bin"
    include "hw.inc"
    include "start.inc"

FXC     equ USER        ; frames to the next x flip
FYC     equ USER + 1
FXV     equ USER + 2    ; the flips as set
FYV     equ USER + 3

init:
    ld hl, VRAM
    ld a, 3
    ld e, 7
    call fill1k
    ; column n: scroll 9n, colour byte 25h * n
    ld hl, SCROLL
    ld b, 32
    xor a
    ld c, a
.col:
    ld (hl), a
    inc hl
    ld (hl), c
    inc hl
    add a, 9
    ld d, a
    ld a, c
    add a, 0x25
    ld c, a
    ld a, d
    djnz .col
    ; no sprites or shots
    ld hl, OBJ + 0x40
    ld bc, 0xC0
    xor a
    call fill
    ld a, 9
    ld (FXC), a
    ld a, 13
    ld (FYC), a
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
    push hl
    ; each column scrolls by its own step, 1 to 4 lines a frame
    ld ix, SCROLL
    ld b, 32
    ld c, 0
.sc:
    ld a, c
    and 3
    inc a
    add a, (ix + 0)
    ld (ix + 0), a
    inc ix
    inc ix
    inc c
    djnz .sc
    pop hl
    push hl
    ; the band: 32 codes at (frame & 31) * 32
    ld a, l
    and 0x1F
    ld l, a
    ld h, 0
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    ld de, VRAM
    add hl, de
    ld b, 32
.band:
    inc (hl)
    inc hl
    djnz .band
    pop hl
    push hl
    ; every 4 frames the colours move on, junk bits and all
    ld a, l
    and 3
    jr nz, .nocol
    ld ix, SCROLL + 1
    ld b, 32
.cl:
    ld a, (ix + 0)
    add a, 0x21
    ld (ix + 0), a
    inc ix
    inc ix
    djnz .cl
.nocol:
    pop hl
    push hl
    call flips
    pop hl
    ; the mirrors: video RAM at 5400h, object RAM to 5FFFh, work RAM at 4400h
    ld a, l
    ld (VRAM + 0x400 + 0x1E5), a
    ld a, (VRAM + 0x400 + 0x0C0)
    ld (VRAM + 0x3E0), a
    ld a, l
    and 0x3F
    ld (OBJ + 0x700 + 0x3E), a      ; column 31's scroll, through the last mirror
    ld a, (OBJ + 0x300 + 0x3D)      ; column 30's colour, read through a mirror
    ld (VRAM + 0x3E1), a
    ld a, l
    ld (RAM + 0x400 + 0x80), a
    ld a, (RAM + 0x80)
    ld (VRAM + 0x3E2), a
    ; reads of nothing give FFh; the inputs read nothing pressed
    ld a, (UNMAP)
    ld (VRAM + 0x3E3), a
    ld a, (0xC123)
    ld (VRAM + 0x3E4), a
    ld a, (IN0)
    ld (VRAM + 0x3E5), a
    ld a, (IN1)
    ld (VRAM + 0x3E6), a
    ld a, (IN2)
    ld (VRAM + 0x3E7), a
    ret

flips:
    ld a, h
    or a
    jr nz, .late
    ld a, l
    cp 40
    jr z, .xon
    cp 70
    jr z, .yon
    cp 100
    jr z, .xoff
    cp 130
    jr z, .yoff
    cp 140
    ret c
.late:
    ld hl, FXC
    dec (hl)
    jr nz, .ny
    ld (hl), 9
    ld a, (FXV)
    xor 1
    ld (FXV), a
    ld (FLIPX), a
.ny:
    ld hl, FYC
    dec (hl)
    ret nz
    ld (hl), 13
    ld a, (FYV)
    xor 1
    ld (FYV), a
    ld (FLIPY), a
    ret
.xon:
    ld a, 1
    jr .x
.xoff:
    xor a
.x:
    ld (FXV), a
    ld (FLIPX), a
    ret
.yon:
    ld a, 1
    jr .y
.yoff:
    xor a
.y:
    ld (FYV), a
    ld (FLIPY), a
    ret

idle:
    ret

    include "end.inc"
