; cArcade's own graphics and PROMs for the Donkey Kong-board test programs (GPL-3.0-only).
; Assembled with sjasmplus into three files:
;   tiles-gfx.bin    8 KB: the tiles' two bitplanes (4 KB each: Jr.'s 3N and 3P). Donkey Kong's 5H and
;                3P ROMs are the first 2 KB of each.
;   sprites-gfx.bin  8 KB: the sprites, as the four 2 KB ROMs hold them (each plane's left halves,
;                then their right halves).
;   proms.bin    768 bytes: the palette's low PROM, its high PROM, then the tile colour codes.
; Everything is pseudo-random, so that every pixel differs from its neighbours and any mistake in
; how the board unpacks a tile, a sprite or a colour shows up in a compared frame; except tile 0
; and sprite 0, which are blank so a program can clear the screen. Tiles 1 and 2 (clear and set
; bits) and 10h-1Fh (hex digits) are random too: they only need to differ.

    DEVICE NOSLOT64K

    org 0x8000
tiles:
seed = 0x2468
    DUP 0x2000
seed = (seed * 25173 + 13849) & 0xFFFF
    DB seed >> 8
    EDUP
    org 0x8000
    DS 8, 0
    org 0x9000
    DS 8, 0

    org 0xA000
sprites:
seed = 0x1357
    DUP 0x2000
seed = (seed * 25173 + 13849) & 0xFFFF
    DB seed >> 8
    EDUP
    org 0xA000
    DS 16, 0
    org 0xA800
    DS 16, 0
    org 0xB000
    DS 16, 0
    org 0xB800
    DS 16, 0

    org 0xC000
proms:
seed = 0x0F0F
    DUP 0x300
seed = (seed * 25173 + 13849) & 0xFFFF
    DB (seed >> 8) & 0x0F
    EDUP

    SAVEBIN "tiles-gfx.bin", tiles, 0x2000
    SAVEBIN "sprites-gfx.bin", sprites, 0x2000
    SAVEBIN "proms.bin", proms, 0x300
