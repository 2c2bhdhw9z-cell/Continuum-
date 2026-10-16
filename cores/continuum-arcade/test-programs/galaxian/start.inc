; The frame loop every Galaxian-board test program shares (GPL-3.0-only). The program supplies:
;   init   - set up, then JP run (BOOTS counts the warm boots before it)
;   frame  - called from the NMI with HL = the frame count (1 at the first one)
;   idle   - called after each NMI, outside it (may delay, to place writes mid-frame)
; VBLANK raises the NMI; the handler drops the line and raises the enable again, as the games do,
; unless NMIKEEP (the line stays high, so no further NMI comes) or NMIOFF (the enable stays off).

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
    ld a, (NMIKEEP)
    or a
    jr nz, .keep
    xor a
    ld (NMIEN), a           ; the line drops
.keep:
    ld a, (NOKICK)
    or a
    jr nz, .nokick
    ld a, (WDOG)            ; kicks the watchdog
.nokick:
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    call frame
    ld a, (NMIKEEP)
    ld b, a
    ld a, (NMIOFF)
    or b
    jr nz, .off
    ld a, 1
    ld (NMIEN), a
.off:
    pop iy
    pop ix
    pop hl
    pop de
    pop bc
    pop af
    retn

boot:
    ; A warm boot (the watchdog's reset) keeps RAM: count it instead of clearing.
    ld hl, (MAGIC)
    ld de, 0x5AA5
    or a
    sbc hl, de
    jr z, .warm
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
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
    ld a, 1
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

; Sets each column's colour byte to its number plus A (the low three bits are the colour).
colours:
    ld hl, SCROLL + 1
    ld b, 32
.c: ld (hl), a
    inc a
    inc hl
    inc hl
    djnz .c
    ret
