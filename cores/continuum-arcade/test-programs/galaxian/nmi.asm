; Galaxian board test: the NMI (GPL-3.0-only, cArcade's own).
; VBLANK raises the NMI line only while the NMI ON latch is set, and the Z80 takes an NMI only as
; the line rises. The main loop doesn't halt here: it counts round (LOOPS, shown at the top) while
; the NMI handler logs each NMI's frame and LOOPS down the screen, so the log shows exactly when
; each NMI came. From frame 15 the handler leaves the line high (no NMI comes) until the main loop
; drops and raises the latch mid-frame; from frame 30 it leaves the latch off until the main loop
; sets it; at 45 the main loop flips the latch six times in a row; from 60 to 90 it drops and
; raises the latch every 128 rounds.

    DEFINE OUTNAME "nmi.bin"
    include "hw.inc"
    include "start.inc"

LOOPS   equ USER        ; 16-bit
PHASE   equ USER + 2
WAITFOR equ USER + 3    ; 16-bit: the LOOPS count the main loop acts at
LOGP    equ USER + 5    ; 16-bit: where the next log entry goes

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, OBJ
    ld bc, 0x100
    xor a
    call fill
    ld a, 1
    call colours
    ld hl, VRAM + 0x40
    ld (LOGP), hl
    ld a, 1
    ld (NMIEN), a
main:
    ld hl, (LOOPS)
    inc hl
    ld (LOOPS), hl
    ld a, h
    ld (VRAM + 0x22), a
    ld a, l
    ld (VRAM + 0x23), a
    ld a, (PHASE)
    or a
    jr z, main
    cp 4
    jr z, .often
    ld de, (WAITFOR)
    or a
    sbc hl, de
    jr nz, main
    ld a, (PHASE)
    cp 1
    jr z, .drop_raise
    cp 2
    jr z, .raise
    ; phase 3: the latch six times in a row
    xor a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
    dec a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
    dec a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
    jr .done
.drop_raise:
    xor a
    ld (NMIKEEP), a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
    jr .done
.raise:
    xor a
    ld (NMIOFF), a
    inc a
    ld (NMIEN), a
.done:
    xor a
    ld (PHASE), a
    jr main
.often:
    ld a, l
    and 0x7F
    jr nz, main
    xor a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
    jr main

frame:
    ; log: the frame, then LOOPS
    ex de, hl
    ld hl, (LOGP)
    ld (hl), e
    inc hl
    ld a, (LOOPS + 1)
    ld (hl), a
    inc hl
    ld a, (LOOPS)
    ld (hl), a
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
    ld a, h
    or a
    ret nz
    ld a, l
    cp 15
    jr z, .keep
    cp 30
    jr z, .off
    cp 45
    jr z, .storm
    cp 60
    jr z, .often
    cp 90
    ret nz
    xor a
    ld (PHASE), a
    ret
.keep:
    ld a, 1
    ld (NMIKEEP), a
    ld de, 4000
    jr .wait
.off:
    ld a, 1
    ld (NMIOFF), a
    ld de, 3000
    ld a, 2
    jr .wait2
.storm:
    ld de, 500
    ld a, 3
    jr .wait2
.often:
    ld a, 4
    ld (PHASE), a
    ret
.wait:
    ld a, 1
.wait2:
    ld (PHASE), a
    ld hl, (LOOPS)
    add hl, de
    ld (WAITFOR), hl
    ret

idle:
    ret

    include "end.inc"
