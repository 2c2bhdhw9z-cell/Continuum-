; Donkey Kong board test: changes in the middle of a frame (GPL-3.0-only, cArcade's own).
; The board draws each line as the beam reaches it, so a change shows from the line after it.
; After each NMI the program waits, then in turn sets palette bank 1, flips the screen, rewrites
; a row of the tile map, sets palette bank 2 and unflips, and switches to the second sprite bank;
; the NMI puts everything back. The waits grow by a different amount each frame (in 16-cycle
; steps, plus 7 and 13 cycles on odd frame bits), so the changes sweep through every point of a
; line. Sprites sit in both banks, written straight into their RAM.

    DEFINE OUTNAME "raster.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld bc, 0x400
    ld e, 0x40
.f: ld (hl), e
    inc e
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .f
    ; Sprites: bank 0 a column of 16, bank 1 a diagonal of 32.
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld ix, SPRRAM
    ld b, 16
    ld c, 40
.s0:
    ld (ix + 0), c
    ld a, c
    ld (ix + 1), a
    ld (ix + 2), b
    ld (ix + 3), 100
    ld a, c
    add a, 13
    ld c, a
    ld de, 4
    add ix, de
    djnz .s0
    ld ix, SPRRAM + 0x200
    ld b, 32
    ld c, 8
.s1:
    ld (ix + 0), c
    ld a, c
    add a, 0x55
    ld (ix + 1), a
    ld a, b
    or 0x80
    ld (ix + 2), a
    ld (ix + 3), c
    ld a, c
    add a, 7
    ld c, a
    ld de, 4
    add ix, de
    djnz .s1
    jp run

frame:
    xor a
    ld (PALBANK), a
    ld (PALBANK + 1), a
    ld (FLIP), a
    ld (SPRBANK), a
    ld hl, VRAM + 12 * 32
    ld bc, 32
    ld a, 0x40
    call fill
    ld a, (FRAMES)
    ld (USER), a
    ret

; Waits 3000 + 16 * (USER * k mod 256) cycles, more or less, for k in C; 7 more on odd frames
; and 13 more on frames with bit 1 set.
wait:
    ld bc, 110
    call delay
    ld a, (USER)
    ld b, a
    xor a
.m: add a, c
    djnz .m
    or a
    jr z, .z
.l: dec a
    jr nz, .l
.z: ld a, (USER)
    rrca
    jr nc, .e
    ld a, 0
.e: ld a, (USER)
    and 2
    jr z, .o
    ld a, (IX + 0)
.o: ret

idle:
    ld c, 3
    call wait
    ld a, 1
    ld (PALBANK), a
    ld c, 5
    call wait
    ld a, 1
    ld (FLIP), a
    ld c, 7
    call wait
    ld hl, VRAM + 12 * 32
    ld b, 32
.r: ld (hl), b
    inc hl
    djnz .r
    ld c, 11
    call wait
    xor a
    ld (PALBANK), a
    ld (FLIP), a
    inc a
    ld (PALBANK + 1), a
    ld c, 13
    call wait
    ld a, 1
    ld (SPRBANK), a
    ret

    include "end.inc"
