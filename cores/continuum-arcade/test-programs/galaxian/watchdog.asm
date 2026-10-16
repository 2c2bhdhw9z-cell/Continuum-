; Galaxian board test: the watchdog (GPL-3.0-only, cArcade's own).
; At frames 30 and 90 (counted on across resets, which keep RAM) the NMI handler stops reading the
; watchdog's address; 8 VBLANKs later the board resets, the program counts the warm boot, sets the
; screen up again and carries on. The frame count and the boot count show at the top, and each
; frame's count is logged down the screen.

    DEFINE OUTNAME "watchdog.bin"
    include "hw.inc"
    include "start.inc"

LOGP    equ USER

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld a, (BOOTS)
    call colours
    ld hl, VRAM + 0x40
    ld (LOGP), hl
    xor a
    ld (NOKICK), a
    jp run

frame:
    ex de, hl
    ld hl, (LOGP)
    ld (hl), e
    inc hl
    ld a, h
    and 0x03
    or VRAM >> 8
    ld h, a
    ld (LOGP), hl
    ex de, hl
    ld a, h
    ld (VRAM + 0x20), a
    ld a, l
    ld (VRAM + 0x21), a
    ld a, (BOOTS)
    ld (VRAM + 0x22), a
    ld a, h
    or a
    ret nz
    ld a, l
    cp 30
    jr z, .stop
    cp 90
    ret nz
.stop:
    ld a, 1
    ld (NOKICK), a
    ret

idle:
    ret

    include "end.inc"
