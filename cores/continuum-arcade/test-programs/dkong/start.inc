; The frame loop every Donkey Kong-board test program shares (GPL-3.0-only). The program supplies:
;   init   - set up, then JP run (BOOTS counts the warm boots before it)
;   frame  - called from the NMI with HL = the frame count (1 at the first one)
;   idle   - called after each NMI, outside it (may delay, to place writes mid-frame)
; VBLANK raises the NMI while the NMI latch is on; the handler turns the latch off (dropping the
; line) and on again, as the games do, unless NMIKEEP (the line stays high, so no further NMI
; comes) or NMIOFF (the latch stays off). With DMAON it has the 8257 copy the sprite list at
; 6900h to the sprites' RAM at 7000h after `frame`, as the games do.

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
    ld (NMIMASK), a         ; the line drops
.keep:
    ld a, (NOKICK)
    or a
    jr nz, .nokick
    ld a, (IN2)             ; kicks the watchdog
.nokick:
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    call frame
    ld a, (DMAON)
    or a
    call nz, dma
    ld a, (NMIKEEP)
    ld b, a
    ld a, (NMIOFF)
    or b
    jr nz, .off
    ld a, 1
    ld (NMIMASK), a
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
    ld bc, 0xBFF
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
    ld (NMIMASK), a
.loop:
    halt
    ld hl, (IDLES)
    inc hl
    ld (IDLES), hl
    call idle
    jr .loop

; The 8257 copies 180h bytes from 6900h to 7000h: channel 0 reads memory into the board's latch,
; channel 1 writes the latch to memory, taking turns (rotating priority), each stopping at its
; count (TC stop). The board swaps the 8257's read and write modes: 40h in a count's top bits
; makes channel 0 read memory, 80h makes channel 1 write it.
dma:
    ld hl, DMA
    ld (hl), low SPRLIST
    ld (hl), high SPRLIST
    inc hl
    ld (hl), 0x7F
    ld (hl), 0x41
    inc hl
    ld (hl), low SPRRAM
    ld (hl), high SPRRAM
    inc hl
    ld (hl), 0x7F
    ld (hl), 0x81
    ld a, 0x53
    ld (DMA + 8), a
    ld a, 1
    ld (DRQ), a
    xor a
    ld (DRQ), a
    ret

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

; Writes A's eight bits as tiles 1 (clear) and 2 (set) at HL onwards, bit 7 first, then moves
; HL on 32 (a row of the map).
bits:
    ld b, 8
.b: rlca
    ld c, 1
    jr nc, .z
    inc c
.z: ld (hl), c
    inc hl
    djnz .b
    ld bc, 24
    add hl, bc
    ret

; Writes A as two hex digits (tiles 10h-1Fh) at HL, HL+1.
hex:
    push af
    rrca
    rrca
    rrca
    rrca
    and 0x0F
    or 0x10
    ld (hl), a
    inc hl
    pop af
    and 0x0F
    or 0x10
    ld (hl), a
    inc hl
    ret
