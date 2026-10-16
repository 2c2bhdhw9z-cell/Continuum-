; Donkey Kong board test: the tile map (GPL-3.0-only, cArcade's own).
; Every one of the 1024 map cells holds a different mix of code and place (all 256 codes appear
; in every colour-code group), and each frame three cells move on a code. The palette bank steps
; every 16 frames (the colour codes' four banks), and the screen flips at 60, back at 120, and
; again at 160. No sprites.

    DEFINE OUTNAME "tiles.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, VRAM
    ld bc, 0x400
    ld e, 0
.f: ld a, l
    add a, a
    add a, l
    xor h
    add a, e
    ld (hl), a
    inc hl
    inc e
    dec bc
    ld a, b
    or c
    jr nz, .f
    jp run

frame:
    ; Three cells move on a code: 13, 101 and 307 cells apart each frame.
    push hl
    ld de, 13
    call bump
    pop hl
    push hl
    ld de, 101
    call bump
    pop hl
    push hl
    ld de, 307
    call bump
    pop hl
    ; The palette bank: frames 16, 32, 48 ... step it.
    ld a, l
    and 15
    jr nz, .nopal
    ld a, l
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
    cp 60
    jr z, .flip
    cp 160
    jr z, .flip
    cp 120
    ret nz
    xor a
    ld (FLIP), a
    ret
.flip:
    ld a, 1
    ld (FLIP), a
    ret

; Adds 1 to the map cell (frame * DE) mod 1024.
bump:
    push hl
    pop bc
    ld hl, 0
.m: add hl, de
    dec bc
    ld a, b
    or c
    jr nz, .m
    ld a, h
    and 3
    or high VRAM
    ld h, a
    inc (hl)
    ret

idle:
    ret

    include "end.inc"
