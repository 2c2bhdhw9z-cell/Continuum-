; Pac-Man board test: every read of 5000h-50FFh, for the boards with a protection chip there
; (GPL-3.0-only, cArcade's own). Each frame it reads all 256 addresses and shows each byte as a
; tile in the playfield, and counts its writes to 5004h the way Make Trax's protection chip counts
; them: 60 a frame (so the chip's table moves on one entry each frame), none from frame 40, a
; single 0 at frame 40 (which switches the chip off), and 60 again from frame 70. The test sets
; the switches frame by frame, which the chip mixes into its answers.

    DEFINE OUTNAME "ports.bin"
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
    ld (hl), 0x05
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jp run

frame:
    ld a, h
    or a
    jr nz, .count
    ld a, l
    cp 40
    jr c, .count
    jr nz, .quiet
    xor a
    ld (0x5004), a
    jr .read
.quiet:
    cp 70
    jr c, .read
.count:
    ld b, 60
    ld a, 1
.w: ld (0x5004), a
    djnz .w
.read:
    ld hl, 0x5000
    ld de, VRAM + 0x040
.r: ld a, (hl)
    ld (de), a
    inc de
    inc l
    jr nz, .r
    ; and a few through the mirrors, into the score rows
    ld a, (0x5084 + 0x8000)
    ld (VRAM + 0x3C2), a
    ld a, (0x50C9 + 0x2000)
    ld (VRAM + 0x3C3), a
    ld a, (0x504E + 0x0100)
    ld (VRAM + 0x3C4), a
    ret

idle:
    ret

    include "end.inc"
