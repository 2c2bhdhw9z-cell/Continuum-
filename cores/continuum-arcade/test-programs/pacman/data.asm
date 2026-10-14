; cArcade's own graphics and PROMs for the Pac-Man board test programs (GPL-3.0-only).
; Assembled with sjasmplus into five files:
;   gfx.bin      8 KB: 5E (256 tiles) then 5F (64 sprites)
;   palette.bin  32 bytes: 7F, the palette PROM
;   lookup.bin   256 bytes: 4A, the colour lookup PROM
;   wave.bin     256 bytes: 1M, the sound PROM (eight 32-step waveforms)
;   timing.bin   256 bytes: 3M, the sound timing PROM (not modelled; zeros)
; The graphics are pseudo-random bytes, so that every pixel of every tile and sprite differs from
; its neighbours and any mistake in how the board unpacks them shows up in a compared frame.

    DEVICE NOSLOT64K

    org 0x8000
gfx:
seed = 0x1234
    DUP 0x2000
seed = (seed * 25173 + 13849) & 0xFFFF
    DB seed >> 8
    EDUP

    org 0xA000
; Every one of the 32 entries different, all eight bits used. Only 0-15 are reachable on Pac-Man.
palette:
    DB 0x00, 0xFF, 0x07, 0x38, 0xC0, 0x3F, 0xC7, 0xF8
    DB 0x5A, 0xA5, 0x96, 0x69, 0x24, 0x92, 0x49, 0xDB
    DB 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80
    DB 0x13, 0x37, 0x7A, 0xB4, 0xE1, 0x2C, 0x8D, 0x6E

    org 0xA100
; Pen 0..3 of each of the 64 colours, low nibble a palette index; high nibbles are junk the board
; ignores. Plenty of zeros, which a sprite shows as see-through whatever its pen.
lookup:
seed = 0x0BAD
    DUP 256
seed = (seed * 25173 + 13849) & 0xFFFF
    DB ((seed >> 8) & 0xF0) | (((seed >> 12) & 1) * ((seed >> 4) & 0x0F))
    EDUP

    org 0xA200
; Eight waveforms of 32 steps (high nibbles junk): a sine, a square, a rising saw, a falling saw,
; a triangle, a 25% pulse, and two pseudo-random ones.
wave:
    DB 0x07,0x08,0x0A,0x0B,0x0C,0x0D,0x0E,0x0E,0x0F,0x0E,0x0E,0x0D,0x0C,0x0B,0x0A,0x08
    DB 0x07,0x06,0x04,0x03,0x02,0x01,0x00,0x00,0x00,0x00,0x00,0x01,0x02,0x03,0x04,0x06
    DUP 16
    DB 0x30
    EDUP
    DUP 16
    DB 0xCF
    EDUP
n = 0
    DUP 32
    DB 0x50 | (n >> 1)
n = n + 1
    EDUP
n = 0
    DUP 32
    DB 0xA0 | (15 - (n >> 1))
n = n + 1
    EDUP
n = 0
    DUP 32
    IF n < 16
    DB n
    ELSE
    DB 31 - n
    ENDIF
n = n + 1
    EDUP
n = 0
    DUP 32
    IF n < 8
    DB 0xFF
    ELSE
    DB 0x10
    ENDIF
n = n + 1
    EDUP
seed = 0x7777
    DUP 64
seed = (seed * 25173 + 13849) & 0xFFFF
    DB seed >> 8
    EDUP

    SAVEBIN "gfx.bin", gfx, 0x2000
    SAVEBIN "palette.bin", palette, 32
    SAVEBIN "lookup.bin", lookup, 256
    SAVEBIN "wave.bin", wave, 256

    org 0xA300
timing:
    DS 256, 0
    SAVEBIN "timing.bin", timing, 256
