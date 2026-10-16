; Donkey Kong Jr. board test (GPL-3.0-only, cArcade's own), on MAME's dkongjr set.
; Jr.'s board has 24 KB of program in three ROMs whose halves and quarters land all over
; 0000h-5FFFh, RAM up to 6FFFh, and a second bank of 256 tiles at 7C80h. The program copies a
; 32-byte table from each 2 KB piece of the ROMs into the tile map, keeps a pattern in RAM at
; 6C00h-6FFFh that it copies a row of each frame, calls code that sits at 5800h, switches the
; tile bank every 25 frames, moves sprites through the DMA, steps the palette bank every 40 frames
; and flips the screen at 80 and back at 130.

    DEFINE JUNIOR
    DEFINE OUTNAME "dkjr.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld hl, VRAM
    ld bc, 0x400
    ld e, 0
.f: ld (hl), e
    inc e
    inc e
    inc e
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .f
    ; A table from each piece of ROM, rows 20-28.
    ld hl, pieces
    ld de, VRAM + 20 * 32
    ld b, 9
.p: push bc
    ld c, (hl)
    inc hl
    ld b, (hl)
    inc hl
    push hl
    push bc
    pop hl
    ld bc, 32
    ldir
    pop hl
    pop bc
    djnz .p
    ; The RAM pattern at 6C00h.
    ld hl, 0x6C00
    ld b, 0
.r: ld a, l
    xor 0x5A
    add a, h
    ld (hl), a
    inc hl
    ld (hl), b
    inc hl
    inc hl
    inc hl
    djnz .r
    ; Sprites.
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRLIST
    ld bc, 0x180
    xor a
    call fill
    ld ix, SPRLIST
    ld b, 12
    ld c, 30
.s: ld (ix + 0), c
    ld a, c
    add a, 0x31
    ld (ix + 1), a
    ld a, c
    rrca
    ld (ix + 2), a
    ld a, c
    add a, a
    ld (ix + 3), a
    ld a, c
    add a, 17
    ld c, a
    ld de, 4
    add ix, de
    djnz .s
    ld a, 1
    ld (DMAON), a
    jp run

pieces:
    DW 0x1000, 0x1800, 0x2000, 0x2800, 0x3000, 0x4000, 0x4800, 0x5000, 0x5800 + 0x100

frame:
    push hl
    ; A row from the RAM pattern: row 4 gets 6C00h + 32 * (frame mod 32).
    ld a, l
    and 31
    ld l, a
    ld h, 0
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    ld de, 0x6C00
    add hl, de
    ld de, VRAM + 4 * 32
    ld bc, 32
    ldir
    pop hl
    push hl
    call far_code
    pop hl
    ; The sprites move.
    ld ix, SPRLIST
    ld b, 12
.m: inc (ix + 0)
    dec (ix + 3)
    dec (ix + 3)
    ld de, 4
    add ix, de
    djnz .m
    ; The tile bank, every 25 frames.
    push hl
    ld de, 25
    call divides
    pop hl
    jr nz, .nobank
    ld a, (GFXBANK_COPY)
    xor 1
    ld (GFXBANK_COPY), a
    ld (GFXBANK), a
.nobank:
    ld a, h
    or a
    ret nz
    ld a, l
    cp 80
    jr z, .flip
    cp 130
    ret nz
    xor a
    ld (FLIP), a
    ret
.flip:
    ld a, 1
    ld (FLIP), a
    ret

GFXBANK_COPY equ USER

; Z if HL is a multiple of DE (HL less than 32768).
divides:
    or a
.d: sbc hl, de
    ret z
    jr nc, .d
    ld a, 1
    or a
    ret

idle:
    ret

    org 0x1000
    DB "TABLE AT 1000H. 5C ROM, PART 3.."
    org 0x1800
    DB "TABLE AT 1800H. 5E ROM, PART 4.."
    org 0x2000
    DB "TABLE AT 2000H. 5C ROM, PART 1.."
    org 0x2800
    DB "TABLE AT 2800H. 5E ROM, PART 2.."
    org 0x3000
    DB "TABLE AT 3000H. 5B ROM, HALF 2.."
    org 0x4000
    DB "TABLE AT 4000H. 5E ROM, PART 1.."
    org 0x4800
    DB "TABLE AT 4800H. 5C ROM, PART 2.."
    org 0x5000
    DB "TABLE AT 5000H. 5E ROM, PART 3.."

    org 0x5800
; The palette bank steps every 40 frames (HL = the frame count).
far_code:
    ld de, 40
    call divides
    ret nz
    ld a, (PALCOUNT)
    inc a
    ld (PALCOUNT), a
    ld (PALBANK), a
    rrca
    ld (PALBANK + 1), a
    ret
PALCOUNT equ USER + 1

    org 0x5900
    DB "TABLE AT 5900H. 5C ROM, PART 4.."

    include "end.inc"
