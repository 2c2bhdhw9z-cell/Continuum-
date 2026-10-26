; Pac-Man board test: Ms. Pac-Man Champion Edition's board (GPL-3.0-only, cArcade's own).
; Its one ROM holds two games, and DSW2 bit 3 picks which shows when the board starts or resets.
; This program is assembled twice, once per game (CHAMPION_ID 1 and 2): it shows which one runs,
; bytes from its 8000h half, and what I/O port 0 reads through the board's switch latch (ports
; 10h and 11h choose DSW2's low bits or the byte written), plus 50C0h, which nothing answers. At
; frame 50 it stops kicking the watchdog, so the board resets and starts the game the switch
; picks then; the test moves the switch at frame 45.

    include "hw.inc"
    include "start.inc"

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x20
    ldir
    ld hl, CRAM
    ld de, CRAM + 1
    ld bc, 0x3FF
    ld (hl), CHAMPION_ID * 3
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jp run

frame:
    ld a, CHAMPION_ID
    ld (VRAM + 0x040), a
    ld a, (0x8100)
    ld (VRAM + 0x041), a
    ld a, (0xB0FF)
    ld (VRAM + 0x042), a
    ld a, l
    out (0x10), a
    in a, (0)
    ld (VRAM + 0x043), a
    ld a, l
    add a, 0x41
    out (0x11), a
    in a, (0)
    ld (VRAM + 0x044), a
    in a, (1)
    ld (VRAM + 0x045), a
    ld a, (DSW2)
    ld (VRAM + 0x046), a
    ld a, (0x50FF)
    ld (VRAM + 0x047), a
    ld a, l
    ld (VRAM + 0x048), a
    ld a, h
    or a
    ret nz
    ld a, l
    cp 50
    ret nz
    ld (NOKICK), a
    ret

idle:
    ret

    org 0x8100
    db CHAMPION_ID * 0x11
    org 0xB0FF
    db CHAMPION_ID + 0x80

    include "end.inc"
    SAVEBIN OUTHI, 0x8000, 0x4000
