; Donkey Kong board test: the watchdog (GPL-3.0-only, cArcade's own).
; The board resets three seconds after the last read of IN2 (MAME's watchdog, armed by the first
; read). The program reads IN2 once as it boots and never again (NOKICK), and shows its boot count
; and the frames since it booted; a warm boot keeps RAM. The 8035's program keeps running the
; whole time and is reset too.

    DEFINE OUTNAME "watchdog.bin"
    include "hw.inc"
    include "start.inc"

init:
    ld a, 1
    ld (NOKICK), a
    ld a, (IN2)
    ld hl, 0
    ld (FRAMES), hl
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    jp run

frame:
    ld hl, VRAM + 10 * 32 + 12
    ld a, (BOOTS)
    call hex
    ld a, (FRAMES + 1)
    call hex
    ld a, (FRAMES)
    call hex
    ret

idle:
    ret

    include "end.inc"
