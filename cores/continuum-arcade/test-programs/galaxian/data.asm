; cArcade's own graphics and colour PROM for the Galaxian-board test programs (GPL-3.0-only).
; Assembled with sjasmplus into two files:
;   gfx.bin      8 KB: the high bitplane (4 KB) then the low one. Galaxian's 1H and 1K ROMs are
;                the first 2 KB of each; Moon Cresta's four ROMs are all of it.
;   palette.bin  32 bytes: the colour PROM.
; The graphics are pseudo-random bytes, so that every pixel of every tile and sprite differs from
; its neighbours and any mistake in how the board unpacks them shows up in a compared frame,
; except tiles 0-3 (sprite 0) and Moon Cresta's tiles 100h-103h (sprite 40h), which are blank so a
; program can clear the screen.

    DEVICE NOSLOT64K

    org 0x8000
gfx:
seed = 0x4321
    DUP 0x2000
seed = (seed * 25173 + 13849) & 0xFFFF
    DB seed >> 8
    EDUP
    org 0x8000
    DS 32, 0
    org 0x8800
    DS 32, 0
    org 0x9000
    DS 32, 0
    org 0x9800
    DS 32, 0

    org 0xA000
; Every one of the 32 entries different, all eight bits used.
palette:
    DB 0x00, 0xFF, 0x07, 0x38, 0xC0, 0x3F, 0xC7, 0xF8
    DB 0x5A, 0xA5, 0x96, 0x69, 0x24, 0x92, 0x49, 0xDB
    DB 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80
    DB 0x13, 0x37, 0x7A, 0xB4, 0xE1, 0x2C, 0x8D, 0x6E

    SAVEBIN "gfx.bin", gfx, 0x2000
    SAVEBIN "palette.bin", palette, 32
