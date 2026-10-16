; Konami-board main test program (GPL-3.0-only, cArcade's own), on any of the four maps (kmap.inc).
; Every frame: the column scrolls, the eight sprites and the eight shots move and change; the
; screen flips every 64 frames; the stars go on and off; the background latches (Scramble's blue
; switch, Turtles' red, green and blue) step through their settings; the inputs (PPI0's three
; ports and PPI1's port C) show as tiles in the top row; on The End's map the protection chip gets
; a nibble a frame through PPI1's port C and its answer shows too. Every 8 frames the sound board
; gets a command; frames 200-219 silence it. Every fourth frame something changes halfway down
; the screen. At frame 150 the program stops kicking the watchdog, the board resets 8 VBLANKs
; later (RAM kept), and the program carries on from where the frame count is.

STEP    equ USER

init:
    xor a
    ld (NOKICK), a
    ld (CTRL), a
    ; PPI1: A and B outputs, C's upper half input and lower half output.
    ld a, 0x88
    ld (PPI1CTL), a
    ld a, 0x9B
    ld (PPI0CTL), a
    ; The tiles: every code somewhere, the boot count in the first column.
    ld hl, VRAM
    ld bc, 0x400
.v: ld a, l
    rrca
    xor h
    add a, c
    ld (hl), a
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .v
    ld a, (BOOTS)
    ld (VRAM + 0x40), a
    ; Each column's colour.
    ld hl, SCROLL + 1
    ld b, 32
    ld a, (BOOTS)
.c: ld (hl), a
    inc a
    inc hl
    inc hl
    djnz .c
    ld a, 1
    ld (STARS), a
    ld (BGEN), a
    jp run

frame:
    ld a, l
    ld (STEP), a
    ; Columns 4-11 scroll at their own speeds.
    ld ix, SCROLL + 8
    ld b, 8
    ld c, a
    ld e, a
.s: ld (ix + 0), e
    ld a, e
    add a, c
    ld e, a
    inc ix
    inc ix
    djnz .s
    ; The sprites.
    ld ix, SPR
    ld b, 8
    ld a, (STEP)
    ld c, a
    ld d, 0x30
.p: ld a, d
    add a, c
    ld (ix + 0), a          ; y
    ld a, c
    rrca
    rrca
    rrca
    add a, d
    ld (ix + 1), a          ; code and flips
    ld a, c
    rrca
    rrca
    rrca
    rrca
    rrca
    add a, b
    ld (ix + 2), a          ; colour
    ld a, d
    sub c
    ld (ix + 3), a          ; x
    ld a, d
    add a, 0x18
    ld d, a
    inc ix
    inc ix
    inc ix
    inc ix
    djnz .p
    ; The shots.
    ld ix, SHOT
    ld b, 8
    ld d, 0
.t: ld a, c
    add a, d
    ld (ix + 1), a          ; y
    ld a, c
    srl a
    add a, d
    add a, 0x40
    ld (ix + 3), a          ; x
    ld a, d
    add a, 0x1C
    ld d, a
    inc ix
    inc ix
    inc ix
    inc ix
    djnz .t
    ; Flips every 64 frames.
    ld a, c
    rlca
    rlca
    and 1
    ld (FLIPX), a
    ld a, c
    rlca
    and 1
    ld (FLIPY), a
    ; Stars off for 16 frames in every 48.
    ld a, c
    and 0x30
    cp 0x30
    ld a, 0
    jr z, .so
    inc a
.so:ld (STARS), a
    ; The background latches.
    ld a, c
    rrca
    rrca
    rrca
    ld (BGEN), a
    IFDEF TURTLES
    ld (BGR), a
    rrca
    ld (BGG), a
    rrca
    ld (BGB), a
    ENDIF
    ld a, c
    ld (COIN), a
    ; The inputs, as tiles.
    ld a, (PPI0A)
    ld (VRAM + 2), a
    ld a, (PPI0B)
    ld (VRAM + 3), a
    ld a, (PPI0C)
    ld (VRAM + 4), a
    ; The protection on The End's map (elsewhere port C's lower half just changes).
    ld a, c
    and 0x0F
    ld (PPI1C), a
    ld a, (PPI1C)
    ld (VRAM + 5), a
    ld a, (PPI0C)
    ld (VRAM + 6), a
    ; A command every 8 frames.
    ld a, c
    and 7
    jr nz, .nocmd
    ld a, c
    call command
.nocmd:
    ; Silence the sound board in frames 200-219.
    ld a, c
    sub 200
    cp 20
    ld a, (CTRL)
    res 4, a
    jr nc, .loud
    set 4, a
.loud:
    ld (CTRL), a
    ld (PPI1B), a
    ; The watchdog: at frame 150 stop kicking it.
    ld a, h
    or a
    ret nz
    ld a, l
    cp 150
    ret nz
    ld a, 1
    ld (NOKICK), a
    ret

idle:
    ld a, (STEP)
    and 3
    cp 1
    ret nz
    ; Halfway down: the background and a column's scroll change.
    ld bc, 0x180
    call delay
    ld a, (STEP)
    rrca
    rrca
    ld (BGEN), a
    IFDEF TURTLES
    ld (BGG), a
    ENDIF
    ld a, (STEP)
    ld (SCROLL + 0x30), a
    ret
