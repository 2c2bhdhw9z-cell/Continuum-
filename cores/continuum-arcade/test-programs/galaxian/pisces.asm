; Pisces board test (GPL-3.0-only, cArcade's own): Galaxian's board with the coin lockout latch
; (6002h) wired to a graphics bank: the bank's byte (MAME keeps all of it) moves every tile code up
; by 256 times it and every sprite code by 64 times it, out of ROMs twice Galaxian's size; on
; Batman Part 2's board only tiles 80h-FFh move, by 256, and the sprites are always the upper
; half's. Every 6 frames the latch takes the next of a list of values; the tiles show every code
; and the sprites a spread of codes, moving. The inputs show as tiles, the stars are on, and the
; screen flips now and then. Run on both boards (tests/galaxian_board.rs).

    DEFINE OUTNAME "pisces.bin"
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

; The bank's values in turn (three bytes each, the first used).
banks:
    DB 0, 0, 0
    DB 1, 0, 0
    DB 0, 0, 0
    DB 1, 0, 0
    DB 2, 0, 0
    DB 3, 0, 0
    DB 0x80, 0, 0
    DB 0xFF, 0, 0
    DB 1, 0, 0
    DB 0, 0, 0
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
    add a, l
    ld l, a
    ld h, 0
    ld de, banks
    add hl, de
    ld a, (hl)
    ld (LOCKOUT), a
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

    include "end.inc"
