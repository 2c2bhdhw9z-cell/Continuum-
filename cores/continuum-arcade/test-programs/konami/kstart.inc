; The frame loop the Konami-board test programs share (GPL-3.0-only). The program supplies init
; (then JP run), frame (from the NMI, HL = the frame count, 1 at the first) and idle (after each
; NMI, outside it). The NMI handler drops and raises the NMI enable as the games do.

    DEVICE NOSLOT64K
    org 0
reset:
    di
    ld sp, STACK
    jp boot

    org 0x66
nmi:
    push af
    push bc
    push de
    push hl
    push ix
    push iy
    xor a
    ld (NMIEN), a
    ld a, (NOKICK)
    or a
    jr nz, .nokick
    ld a, (WDOG)
.nokick:
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    call frame
    ld a, 1
    ld (NMIEN), a
    pop iy
    pop ix
    pop hl
    pop de
    pop bc
    pop af
    retn

boot:
    ld hl, (MAGIC)
    ld de, 0x5AA5
    or a
    sbc hl, de
    jr z, .warm
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x7FF
    ld (hl), 0
    ldir
    ld hl, 0x5AA5
    ld (MAGIC), hl
    jp init
.warm:
    ld hl, BOOTS
    inc (hl)
    jp init

run:
    ; After the watchdog's reset the NMI line is still high (the latch keeps its setting), so
    ; it has to drop before it can rise again.
    xor a
    ld (NMIEN), a
    inc a
    ld (NMIEN), a
.loop:
    halt
    call idle
    jr .loop

; Waits about BC * 26 + 20 T-states (BC at least 1).
delay:
    dec bc
    ld a, b
    or c
    jr nz, delay
    ret

; Fills BC bytes (at least 2) at HL with A.
fill:
    ld (hl), a
    ld d, h
    ld e, l
    inc de
    dec bc
    ldir
    ret

; Sends the sound board a command: A to the latch, then bit 3 of port B high and low again.
command:
    ld (PPI1A), a
    ld a, (CTRL)
    or 0x08
    ld (PPI1B), a
    and 0xF7
    ld (PPI1B), a
    ld (CTRL), a
    ret
