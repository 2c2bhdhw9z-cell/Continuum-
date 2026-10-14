; Pac-Man board test: the VBLANK interrupt (GPL-3.0-only, cArcade's own).
; The board puts the byte last written to I/O port 0 on the bus when the Z80 takes the
; interrupt, and holds the interrupt until the game clears bit 0 of 5000h. This program:
;   frames 1-39:    mode 2 through five vectors (00h, 02h, 7Eh, FEh and the odd 81h), one
;                   handler each;
;   frames 40-79:   mode 1 (RST 38h, whatever the vector);
;   frames 80-119:  mode 0, the vector being the instruction: RST 08h, 10h, 18h and 38h in turn;
;   frames 120-159: mode 2 with the handler re-enabling interrupts before clearing 5000h, so the
;                   held interrupt comes straight back in, three deep, before it is cleared;
;   then mode 2 through the five vectors again.
; Each handler counts its own entries; the counts and the frame number are shown as tiles.

    DEVICE NOSLOT64K
    include "hw.inc"

CNT     equ USER        ; 8 counters, one per handler
NEST    equ USER + 8    ; how deep the held-interrupt handler is
MODE    equ USER + 9

    org 0
    di
    ld sp, STACK
    jp init
    org 0x08
    jp h_rst08
    org 0x10
    jp h_rst10
    org 0x18
    jp h_rst18
    org 0x38
    jp h_rst38

    org 0x40
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
    ld (hl), 0x07
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ld a, 0x3E
    ld i, a
    im 2
    xor a
    out (0), a
    ld a, 1
    ld (IRQEN), a
    ei
.loop:
    halt
    jr .loop

; Each handler: save, clear the interrupt, count, do the frame's work, re-enable.
h_vec00: push af
    push hl
    ld hl, CNT + 0
    jr handler
h_vec02: push af
    push hl
    ld hl, CNT + 1
    jr handler
h_vec7e: push af
    push hl
    ld hl, CNT + 2
    jr handler
h_vecfe: push af
    push hl
    ld hl, CNT + 3
    jr handler
h_vec81: push af
    push hl
    ld hl, CNT + 4
    jr handler
h_rst38: push af
    push hl
    ld hl, CNT + 5
    jr handler
h_rst08: push af
    push hl
    ld hl, CNT + 6
    jr handler
h_rst10: push af
    push hl
    ld hl, CNT + 7
    jr handler
h_rst18: push af
    push hl
    ld hl, CNT + 7
    inc (hl)
    jr handler

handler:
    xor a
    ld (IRQEN), a
    ld (WDOG), a
    inc (hl)
    push bc
    push de
    call work
    pop de
    pop bc
    ld a, 1
    ld (IRQEN), a
    pop hl
    pop af
    ei
    reti

; Frames 120-159: interrupts back on before the held interrupt is cleared.
h_hold:
    push af
    push hl
    ld hl, NEST
    inc (hl)
    ld a, (hl)
    cp 3
    jr nc, .clear
    ei
    nop                     ; the interrupt comes back in after this instruction
    jr .out
.clear:
    xor a
    ld (IRQEN), a
    ld (WDOG), a
    push bc
    push de
    call work
    pop de
    pop bc
    ld a, 1
    ld (IRQEN), a
.out:
    ld hl, CNT + 0
    inc (hl)
    ld hl, NEST
    dec (hl)
    pop hl
    pop af
    ei
    reti

; Counts the frame, shows the counters, and picks the next interrupt's mode and vector.
work:
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    ld a, l
    ld (VRAM + 0x3C4), a
    ld a, h
    ld (VRAM + 0x3C5), a
    ld hl, CNT
    ld de, VRAM + 0x040
    ld bc, 9
    ldir
    ld hl, (FRAMES)
    ld a, h
    or a
    jr nz, .m2
    ld a, l
    cp 40
    jr c, .m2
    cp 80
    jr c, .m1
    cp 120
    jr c, .m0
    cp 160
    jr c, .hold
.m2:
    im 2
    ld a, l
    and 7
    cp 5
    jr c, .v
    sub 5
.v: ld e, a
    ld d, 0
    ld hl, vectors
    add hl, de
    ld a, (hl)
    out (0), a
    ret
.m1:
    im 1
    ld a, l
    out (0), a              ; ignored in mode 1
    ret
.m0:
    im 0
    ld a, l
    and 3
    ld e, a
    ld d, 0
    ld hl, rsts
    add hl, de
    ld a, (hl)
    out (0), a
    ret
.hold:
    im 2
    ld a, 0x10
    out (0), a
    ret

vectors: db 0x00, 0x02, 0x7E, 0xFE, 0x81
rsts:    db 0xCF, 0xD7, 0xDF, 0xFF

    org 0x3E00
    dw h_vec00
    dw h_vec02
    org 0x3E10
    dw h_hold
    org 0x3E7E
    dw h_vec7e
    org 0x3E81
    dw h_vec81
    org 0x3EFE
    dw h_vecfe

    SAVEBIN "irq.bin", 0, 0x4000
