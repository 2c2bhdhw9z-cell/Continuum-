; Pac-Man board test: how a board's ROMs are wired (GPL-3.0-only, cArcade's own).
; For the boards that scramble their program or graphics ROMs (Eyes, Woodpecker, Ponpoko, Crush
; Roller 5, the encrypted Ms. Pac-Man bootlegs) or swap the palette's guns: code and tables all over
; the program's 16 KB (and on boards with ROM at 8000h, there too), every tile code and colour on
; the screen, and the eight sprites stepping through all 64 sprite codes, both flips and every
; colour. Whatever wiring a board has, the test lays the program and graphics out scrambled the
; same way, so the board must unscramble them exactly to show MAME's picture.

    DEFINE OUTNAME "wiring.bin"
    include "hw.inc"
    include "start.inc"

init:
    jp init2

frame:
    jp frame2

idle:
    ret

; ---- code and tables in 1000h-1FFFh (the 74298 bootleg scrambles this ROM) ------------------
    org 0x1000
init2:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, TILES
    ld de, VRAM
    ld bc, 0x400
    ldir
    ld hl, COLOURS
    ld de, CRAM
    ld bc, 0x400
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ; a board with ROM at 8000h has its own set-up there
    ld a, (0x8000)
    cp 0x55
    call z, 0x8010
    jp run

frame2:
    push hl
    ; the eight sprites: codes (frame + 9i) & 63, flips from the frame, colour 3i + frame / 4
    ld ix, SPRATTR
    ld iy, SPRXY
    ld b, 0
.spr:
    ld a, b
    add a, a
    add a, a
    add a, a
    add a, b                ; 9i
    add a, l
    and 0x3F
    add a, a
    add a, a
    ld c, a
    ld a, l
    rrca
    rrca
    rrca
    and 3
    or c
    ld (ix + 0), a
    ld a, b
    add a, a
    add a, b
    ld c, a
    ld a, l
    rrca
    rrca
    and 0x3F
    add a, c
    ld (ix + 1), a
    ; positions: across and down the screen, moving each frame
    ld a, b
    rlca
    rlca
    rlca
    rlca
    rlca
    add a, l
    add a, 24
    ld (iy + 0), a
    ld a, l
    add a, a
    add a, b
    add a, b
    add a, b
    add a, 40
    ld (iy + 1), a
    inc ix
    inc ix
    inc iy
    inc iy
    inc b
    ld a, b
    cp 8
    jr nz, .spr
    pop hl
    push hl
    ; bytes from the ROM's other quarters, into the score rows
    ld h, 0
    ld de, TABLE3
    add hl, de
    ld a, (hl)
    ld (VRAM + 0x3C2), a
    ld de, 0x0400
    add hl, de
    ld a, (hl)
    ld (VRAM + 0x3C3), a
    pop hl
    push hl
    ld a, (0x8000)
    cp 0x55
    call z, 0x8020
    pop hl
    ; the screen flip, on for frames 60 to 89
    ld a, l
    cp 60
    jr c, .noflip
    cp 90
    jr nc, .noflip
    ld a, 1
    jr .setflip
.noflip:
    xor a
.setflip:
    ld (FLIP), a
    ret

; ---- tables, in 2000h-2FFFh and 3000h-3FFFh -----------------------------------------------
    org 0x2000
TILES:
c = 0
    DUP 1024
    db (c * 73 + 29 + (c >> 5)) & 255
c = c + 1
    EDUP
COLOURS:
c = 0
    DUP 1024
    db (c * 11 + (c >> 3)) & 255
c = c + 1
    EDUP

    org 0x3000
TABLE3:
c = 0
    DUP 0xFF0
    db (c * 151 + 77) & 255
c = c + 1
    EDUP

; ---- the second 16 KB, on boards with ROM at 8000h ------------------------------------------
    org 0x8000
    db 0x55
    org 0x8010
    ; set-up: 256 bytes from 9000h into the top of the playfield
    ld hl, 0x9000
    ld de, VRAM + 0x100
    ld bc, 0x100
    ldir
    ret
    org 0x8020
    ; each frame: a byte from 9800h + frame into the score rows
    ld h, 0x98
    ld a, (hl)
    ld (VRAM + 0x3C4), a
    ld a, l
    ld (VRAM + 0x3C5), a
    ret
    org 0x9000
c = 0
    DUP 0x1000
    db (c * 97 + 13 + (c >> 8)) & 255
c = c + 1
    EDUP
    org 0xA000
c = 0
    DUP 0x1FF0
    db (c * 59 + 5) & 255
c = c + 1
    EDUP

    SAVEBIN "wiring-hi.bin", 0x8000, 0x4000
    include "end.inc"
