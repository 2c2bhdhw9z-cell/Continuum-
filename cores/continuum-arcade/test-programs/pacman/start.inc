; The frame loop every Pac-Man board test program shares (GPL-3.0-only). The program supplies:
;   init   - set up, then JP run
;   frame  - called from the VBLANK interrupt with HL = the frame count (1 at the first one)
;   idle   - called after each interrupt, outside it (may delay, to place writes mid-frame)
; Interrupts are mode 2 with vector FEh, so the handler's address is the word at 3FFEh
; (end.inc puts it there). The handler kicks the watchdog every frame.

    DEVICE NOSLOT64K
    org 0
reset:
    di
    ld sp, STACK
    jp init

run:
    ld a, 0x3F
    ld i, a
    im 2
    ld a, 0xFE
    out (0), a
    ld a, 1
    ld (IRQEN), a
    ei
.loop:
    halt
    call idle
    jr .loop

vblank:
    push af
    push bc
    push de
    push hl
    push ix
    push iy
    xor a
    ld (IRQEN), a           ; clears the held interrupt
    ld a, (NOKICK)
    or a
    jr nz, .nokick
    ld (WDOG), a
.nokick:
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    call frame
    ld a, 1
    ld (IRQEN), a
    pop iy
    pop ix
    pop hl
    pop de
    pop bc
    pop af
    ei
    reti
