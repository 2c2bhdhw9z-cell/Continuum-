; Jump Bug board test (GPL-3.0-only, cArcade's own): Galaxian's video moved and extended. Five
; graphics bank latches (6002h-6006h) move tiles 80h-BFh and sprites 20h-2Fh into the rest of 768
; tiles; every 6 frames the latches take the next of a list of settings while the tiles show every
; code and the sprites codes on both sides of 20h-2Fh, moving. Also: Scramble's two-dot shots, the
; stars (kept out of the status area), tones on the AY-3-8910, the protection ROM's answers and the
; 8000h-AFFFh program ROM as tiles, the inputs, and the screen flipping now and then.

    DEFINE OUTNAME "jumpbug.bin"
    include "jumpbug.inc"
    include "hw.inc"
    include "start.inc"
STEP    equ USER

init:
    ld hl, VRAM
    ld b, 0
    ld a, 0x70
.v: ld (hl), a
    inc a
    inc hl
    ld (hl), a
    add a, 0x3F
    inc hl
    djnz .v
    ld hl, VRAM + 0x200
    ld b, 0
    ld a, 0x80
.w: ld (hl), a
    inc a
    inc hl
    ld (hl), a
    inc a
    inc hl
    djnz .w
    ld hl, OBJ
    ld bc, 0x100
    xor a
    call fill
    ld a, 2
    call colours
    ld hl, sprite_tab
    ld de, SPR
    ld bc, 32
    ldir
    ld a, 1
    ld (STARS), a
    jp run

sprite_tab:
    DB 40, 0x1E, 0x01, 20
    DB 60, 0x1F, 0x02, 50
    DB 80, 0x20, 0x03, 80
    DB 100, 0x67, 0x04, 110
    DB 120, 0xAB, 0x05, 140
    DB 140, 0x2F, 0x06, 170
    DB 160, 0x30, 0x07, 200
    DB 180, 0x25, 0x01, 230

; The bank settings in turn: latches 0 to 4.
banks:
    DB 0, 0, 0, 0, 0
    DB 0, 0, 1, 0, 0
    DB 1, 0, 1, 0, 1
    DB 0, 1, 1, 1, 0
    DB 1, 1, 1, 0, 1
    DB 1, 1, 0, 1, 1
    DB 2, 0, 1, 0, 0
    DB 0, 3, 1, 0, 1
    DB 0xFF, 0xFF, 0x81, 0, 0xFE
    DB 0, 0, 0xFF, 0, 0
NBANKS  equ 10

frame:
    push hl
    ; the sprites drift
    ld ix, SPR
    ld b, 8
.s: inc (ix + 0)
    dec (ix + 3)
    dec (ix + 3)
    ld de, 4
    add ix, de
    djnz .s
    pop hl
    push hl
    ; every 6 frames, the next bank setting
    ld a, (STEP)
    inc a
    cp 6
    jr c, .same
    xor a
.same:
    ld (STEP), a
    jr nz, .nobank
    ld a, (BANKN)
    inc a
    cp NBANKS
    jr c, .bn
    xor a
.bn:
    ld (BANKN), a
    ld l, a
    add a, a
    add a, a
    add a, l
    ld l, a
    ld h, 0
    ld de, banks
    add hl, de
    ld de, GFXBANK
    ld bc, 5
    ldir
.nobank:
    pop hl
    ; the inputs as tile codes, and the flips now and then
    ld a, (IN0)
    ld (VRAM + 0x3C2), a
    ld a, (IN1)
    ld (VRAM + 0x3C4), a
    ld a, (IN2)
    ld (VRAM + 0x3C6), a
    ld a, (UNMAP)
    ld (VRAM + 0x3C8), a
    ; the protection's answers, the high ROM, and RAM's top half
    ld a, (0xB114)
    ld (VRAM + 0x3CA), a
    ld a, (0xB118)
    ld (VRAM + 0x3CC), a
    ld a, (0xB214)
    ld (VRAM + 0x3CE), a
    ld a, (0xB235)
    ld (VRAM + 0x3D0), a
    ld a, (0xB000)
    ld (VRAM + 0x3D2), a
    ld a, (0x8123)
    ld (VRAM + 0x3D4), a
    ld a, (0xA7FF)
    ld (VRAM + 0x3D6), a
    ld a, (0xB800)
    ld (VRAM + 0x3D8), a
    ld a, l
    ld (0x4600), a
    ld a, (0x4600)
    ld (VRAM + 0x3DA), a
    call shots_and_tones
    ld a, l
    and 0x1F
    ret nz
    ld a, l
    rrca
    rrca
    rrca
    rrca
    rrca
    ld (FLIPX), a
    rrca
    ld (FLIPY), a
    ret

BANKN   equ USER + 1

idle:
    ret


; Shots across the screen, and the AY's tones and noise moving with the frame count in L.
shots_and_tones:
    push hl
    ld ix, SHOT
    ld b, 8
    ld c, l
.sh:
    ld a, c
    add a, 30
    ld (ix + 1), a
    ld a, c
    add a, a
    ld (ix + 3), a
    ld de, 4
    add ix, de
    ld a, c
    add a, 23
    ld c, a
    djnz .sh
    pop hl
    push hl
    ; AY: tone A's period from the frame, B fixed, noise, the mixer, the amplitudes
    xor a
    ld (AYADDR), a
    ld a, l
    ld (AYDATA), a
    ld a, 1
    ld (AYADDR), a
    ld a, 1
    ld (AYDATA), a
    ld a, 2
    ld (AYADDR), a
    ld a, 0x80
    ld (AYDATA), a
    ld a, 6
    ld (AYADDR), a
    ld a, l
    and 0x1F
    ld (AYDATA), a
    ld a, 7
    ld (AYADDR), a
    ld a, l
    and 0x08
    or 0x34
    ld (AYDATA), a
    ld a, 8
    ld (AYADDR), a
    ld a, l
    rrca
    rrca
    and 0x0F
    ld (AYDATA), a
    ld a, 9
    ld (AYADDR), a
    ld a, 0x0A
    ld (AYDATA), a
    pop hl
    ret

    include "end.inc"
