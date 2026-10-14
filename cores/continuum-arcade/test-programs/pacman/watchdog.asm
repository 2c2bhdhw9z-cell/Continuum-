; Pac-Man board test: the watchdog (GPL-3.0-only, cArcade's own).
; Each boot counts itself in RAM, which a reset keeps. It kicks the watchdog (50C0h) for
; 20 + 7 × boots frames with a tone playing and the screen flipped, then stops kicking but keeps
; taking interrupts. Sixteen VBLANKs later the watchdog resets the board: the Z80 starts again at
; 0000h, and the latch's reset turns the sound, the flip and the interrupt off. The boot count,
; the frame count and the last frame before the reset are shown as tiles.

    DEFINE OUTNAME "watchdog.bin"
    include "hw.inc"
    include "start.inc"

KICKS   equ USER        ; frames left to kick the watchdog
LASTF   equ USER + 2    ; 2 bytes: the frame count when the kicking stopped

init:
    ld hl, (MAGIC)
    ld de, 0x5AA5
    or a
    sbc hl, de
    jr z, .warm
    ; power on: set RAM up
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, 0x5AA5
    ld (MAGIC), hl
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x31
    ldir
    ld hl, CRAM
    ld de, CRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x0B
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    jr .go
.warm:
    ld hl, BOOTS
    inc (hl)
.go:
    ld hl, 0
    ld (FRAMES), hl
    xor a
    ld (NOKICK), a
    ld a, (BOOTS)
    ld (VRAM + 0x3C2), a
    ld b, a
    add a, a
    add a, a
    add a, a
    sub b                   ; 7 × boots
    add a, 20
    ld (KICKS), a
    ; the sound on (every voice is at volume 0 here), a tone on voice 0, the screen flipped
    ld a, 1
    ld (SNDEN), a
    ld (FLIP), a
    ld a, 2
    ld (SOUND + S_WAVE0), a
    ld a, (BOOTS)
    add a, 3
    ld (SOUND + S_FREQ0 + 3), a
    ld a, 9
    ld (SOUND + S_VOL0), a
    jp run

frame:
    ld a, l
    ld (VRAM + 0x3C4), a
    ld a, (KICKS)
    or a
    jr nz, .kicking
    ; 14 frames after the last kick, two before the reset, the tone stops: the reset switches
    ; the sound off, which MAME applies from its sound stream's last update, so it is only
    ; compared while every voice is silent
    ld de, (LASTF)
    or a
    sbc hl, de
    ld a, l
    cp 14
    ret nz
    xor a
    ld (SOUND + S_VOL0), a
    ret
.kicking:
    dec a
    ld (KICKS), a
    ret nz
    ; the last kick was this frame's; from the next one on the frame loop stops kicking
    ld (LASTF), hl
    ld a, l
    ld (VRAM + 0x3C6), a
    ld a, 1
    ld (NOKICK), a
    ret

idle:
    ret

    include "end.inc"
