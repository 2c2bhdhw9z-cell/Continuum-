; The Galaxian board's sound latches, played from a script (GPL-3.0-only, cArcade's own): the
; tone at four pitches with each VOL1/VOL2 setting, each background oscillator on its own and
; with the LFO bits set, HIT and FIRE and their tails, with quiet between. Each line is a frame
; (counted from the first NMI), a latch's address and the value written to it, right after that
; frame's VBLANK. Included by sound.asm (Galaxian's map) and mcsound.asm (Moon Cresta's).
; tests/galaxian/sound.rs keeps the same segments (28 of them, 869 frames).

PTR     equ USER

    MACRO W frame, addr, value
    DW frame, addr
    DB value
    ENDM

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, script
    ld (PTR), hl
    jp run

; HL = the frame: every line of the script for it, in order.
frame:
    ld ix, (PTR)
.next:
    ld a, (ix + 0)
    cp l
    ret nz
    ld a, (ix + 1)
    cp h
    ret nz
    ld e, (ix + 2)
    ld d, (ix + 3)
    ld a, (ix + 4)
    ld (de), a
    ld de, 5
    add ix, de
    ld (PTR), ix
    jr .next

idle:
    ret

script:
    W 2, SND + 0, 0x00
    W 2, SND + 1, 0x00
    W 2, SND + 2, 0x00
    W 2, SND + 3, 0x00
    W 2, SND + 4, 0x00
    W 2, SND + 5, 0x00
    W 2, SND + 6, 0x00
    W 2, SND + 7, 0x00
    W 2, LFO + 0, 0x00
    W 2, LFO + 1, 0x00
    W 2, LFO + 2, 0x00
    W 2, LFO + 3, 0x00
    W 2, PITCH, 0xFF
    W 10, SND + 6, 0x00
    W 10, SND + 7, 0x00
    W 10, PITCH, 0x00
    W 34, SND + 6, 0x01
    W 34, SND + 7, 0x00
    W 34, PITCH, 0x00
    W 58, SND + 6, 0x00
    W 58, SND + 7, 0x01
    W 58, PITCH, 0x00
    W 82, SND + 6, 0x01
    W 82, SND + 7, 0x01
    W 82, PITCH, 0x00
    W 106, SND + 6, 0x00
    W 106, SND + 7, 0x00
    W 106, PITCH, 0x60
    W 130, SND + 6, 0x01
    W 130, SND + 7, 0x00
    W 130, PITCH, 0x60
    W 154, SND + 6, 0x00
    W 154, SND + 7, 0x01
    W 154, PITCH, 0x60
    W 178, SND + 6, 0x01
    W 178, SND + 7, 0x01
    W 178, PITCH, 0x60
    W 202, SND + 6, 0x00
    W 202, SND + 7, 0x00
    W 202, PITCH, 0xA0
    W 226, SND + 6, 0x01
    W 226, SND + 7, 0x00
    W 226, PITCH, 0xA0
    W 250, SND + 6, 0x00
    W 250, SND + 7, 0x01
    W 250, PITCH, 0xA0
    W 274, SND + 6, 0x01
    W 274, SND + 7, 0x01
    W 274, PITCH, 0xA0
    W 298, SND + 6, 0x00
    W 298, SND + 7, 0x00
    W 298, PITCH, 0xD0
    W 322, SND + 6, 0x01
    W 322, SND + 7, 0x00
    W 322, PITCH, 0xD0
    W 346, SND + 6, 0x00
    W 346, SND + 7, 0x01
    W 346, PITCH, 0xD0
    W 370, SND + 6, 0x01
    W 370, SND + 7, 0x01
    W 370, PITCH, 0xD0
    W 394, SND + 6, 0x00
    W 394, SND + 7, 0x00
    W 394, PITCH, 0xFF
    W 414, SND + 0, 0x01
    W 474, SND + 0, 0x00
    W 474, SND + 1, 0x01
    W 534, SND + 1, 0x00
    W 534, SND + 2, 0x01
    W 594, SND + 2, 0x00
    W 594, LFO + 0, 0x01
    W 594, LFO + 1, 0x01
    W 594, LFO + 2, 0x01
    W 594, LFO + 3, 0x01
    W 594, SND + 0, 0x01
    W 654, LFO + 0, 0x00
    W 654, LFO + 2, 0x00
    W 654, SND + 1, 0x01
    W 654, SND + 2, 0x01
    W 714, SND + 0, 0x00
    W 714, SND + 1, 0x00
    W 714, SND + 2, 0x00
    W 714, LFO + 0, 0x00
    W 714, LFO + 1, 0x00
    W 714, LFO + 2, 0x00
    W 714, LFO + 3, 0x00
    W 734, SND + 3, 0x01
    W 754, SND + 3, 0x00
    W 799, SND + 5, 0x01
    W 819, SND + 5, 0x00
    DW 0xFFFF
