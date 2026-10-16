; Donkey Kong board test: the NMI latch (GPL-3.0-only, cArcade's own).
; The Z80's NMI is edge-triggered and VBLANK only raises the line while the latch at 7D84h is on;
; turning the latch off drops the line. The program runs a busy loop (no HALT) that shows the NMI
; count, its own count and the phase it is in, and steps through:
;   phase 0: NMIs as usual, until the 20th;
;   phase 1: the handler leaves the line high (NMIKEEP): no more NMIs, though the latch stays on;
;            the loop rewrites the latch with 1 a few times (no edge, nothing), then off and on;
;   phase 2: the handler leaves the latch off (NMIOFF); the loop turns it on in the middle of a
;            frame, and the next VBLANK brings an NMI;
;   phase 3: the loop turns the latch on and off and on within a frame, then leaves it.

    DEFINE OUTNAME "nmi.bin"
    include "hw.inc"
    include "start.inc"

PHASE   equ USER
COUNT   equ USER + 1        ; 2 bytes

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld a, 1
    ld (NMIMASK), a
loop:
    ld hl, (COUNT)
    inc hl
    ld (COUNT), hl
    ; Show the NMI count, the loop count's top byte and the phase.
    ld hl, VRAM + 10 * 32 + 12
    ld a, (FRAMES)
    call hex
    ld a, (COUNT + 1)
    call hex
    ld a, (PHASE)
    call hex
    ld a, (PHASE)
    or a
    jr z, .p0
    dec a
    jr z, .p1
    dec a
    jr z, .p2
    dec a
    jr z, .p3
    jp loop
.p0:
    ld a, (FRAMES)
    cp 20
    jp c, loop
    ld a, 1
    ld (NMIKEEP), a
    ld (PHASE), a
    ld hl, 0
    ld (COUNT), hl
    jp loop
.p1:
    ; Every 1024 loops rewrite the latch on (no edge); at 8 x 1024, off then on.
    ld a, (COUNT)
    or a
    jp nz, loop
    ld a, (COUNT + 1)
    and 0x03
    jp nz, loop
    ld a, (COUNT + 1)
    cp 0x20
    ld a, 1
    ld (NMIMASK), a
    jp c, loop
    xor a
    ld (NMIKEEP), a
    ld (NMIMASK), a
    inc a
    ld (NMIMASK), a
    ld (NMIOFF), a
    ld a, 2
    ld (PHASE), a
    ld hl, 0
    ld (COUNT), hl
    jp loop
.p2:
    ld hl, (COUNT)
    ld a, h
    cp 0x10
    jp nz, loop
    ld a, l
    or a
    jp nz, loop
    ld a, 1
    ld (NMIMASK), a
    xor a
    ld (NMIOFF), a
    ld a, 3
    ld (PHASE), a
    ld hl, 0
    ld (COUNT), hl
    jp loop
.p3:
    ld hl, (COUNT)
    ld a, h
    cp 0x10
    jp nz, loop
    ld a, l
    or a
    jp nz, loop
    ld a, 1
    ld (NMIMASK), a
    xor a
    ld (NMIMASK), a
    inc a
    ld (NMIMASK), a
    ld a, 4
    ld (PHASE), a
    jp loop

frame:
    ret

idle:
    ret

    include "end.inc"
