; Pac-Man board test: the interrupt vector, through the boards that change it on its way to the Z80
; (GPL-3.0-only, cArcade's own). Piranha's and Naughty Mouse's sync bus controllers and Ms. Pac-Man
; II's board pass some vectors on as others. Each frame the program writes the next vector of a
; list to port 0 (and from frame 100 to port 37h, which only Ms. Pac-Man II's board listens to), and
; the mode 2 table has an entry for every vector the list can turn into, each a stub that notes
; which one ran before going on to the usual VBLANK handler. The picture shows the history.
; FAh and FBh's entries overlap (3FFAh-3FFCh), so their handlers sit at 2020h and 2120h.

    DEFINE OUTNAME "vectors.bin"
    include "hw.inc"
    include "start.inc"

LAST    equ USER            ; the stub that ran last
HIST    equ USER + 1        ; where the next history tile goes (2 bytes)

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
    ld (hl), 0x09
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ld hl, VRAM + 0x040
    ld (HIST), hl
    jp run

frame:
    ; note which entry ran
    ld de, (HIST)
    ld a, (LAST)
    ld (de), a
    inc de
    ld a, d
    cp high (VRAM + 0x3C0)
    jr c, .ok
    ld de, VRAM + 0x040
.ok:
    ld (HIST), de
    ld a, 0xEE
    ld (LAST), a
    ; the next vector, at port 0 or (from frame 100) port 37h
    ld a, l
    and 7
    ld e, a
    ld d, 0
    push hl
    ld hl, LIST
    add hl, de
    ld a, (hl)
    pop hl
    ld c, a
    ld a, h
    or a
    jr nz, .p37
    ld a, l
    cp 100
    jr nc, .p37
    ld a, c
    out (0), a
    ret
.p37:
    ld a, c
    out (0x37), a
    ret

idle:
    ret

LIST:
    db 0xFE, 0xFA, 0xBF, 0xC6, 0xFB, 0x78, 0x3C, 0x40

stub macro id
    push af
    ld a, id
    ld (LAST), a
    pop af
    jp vblank
    endm

    org 0x1100
h3c: stub 0x3C
h40: stub 0x40
h78: stub 0x78
hbf: stub 0xBF
hc6: stub 0xC6
    org 0x2020
hfa: stub 0xFA
    org 0x2120
hfb: stub 0xFB

    org 0x3F3C
    dw h3c
    org 0x3F40
    dw h40
    org 0x3F78
    dw h78
    org 0x3FBF
    dw hbf
    org 0x3FC6
    dw hc6
    org 0x3FFA
    db 0x20, 0x20, 0x21     ; FAh -> 2020h, FBh -> 2120h

    include "end.inc"
