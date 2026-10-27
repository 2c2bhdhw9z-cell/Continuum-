; hsc: the High Score Cartridge's memory as a game sees it, with our own stand-in for its ROM
; (byte i is i * 7 + i / 256, made by tests/mame/a78.py and tests/mame_frames.rs): its ROM at
; $3000-$3FFF and its 2 KB of RAM at $1000-$17FF, fresh ($FF) at power-on. The program reads
; both, fills the RAM and reads it back (the readings show as bands), and has MARIA draw the
; bottom zone's graphics straight from the HSC's ROM.

        processor 6502
        include "hw.inc"

sum     = $70
ptr     = $72
GFXDL   = $1890

        org $8000
reset:
        sei
        cld
        ldx #$FF
        txs
        lda #DMA_OFF
        sta CTRL
        lda #0
        tax
clr:    sta $1800,x
        sta $1900,x
        inx
        bne clr
        ; the ROM: its first and last bytes, and all 4 KB added up
        lda $3000
        sta values+0
        lda $3FFF
        sta values+1
        lda #$30
        ldx #16
        jsr add_pages
        lda sum
        sta values+2
        lda sum+1
        sta values+3
        ; the RAM as it comes, then filled (byte i: i's low byte XOR its page) and read back
        lda $1000
        sta values+4
        lda $17FF
        sta values+5
        lda #0
        sta ptr
        lda #$10
        sta ptr+1
        ldx #8
fill:   ldy #0
fill2:  tya
        eor ptr+1
        sta (ptr),y
        iny
        bne fill2
        inc ptr+1
        dex
        bne fill
        lda #$10
        ldx #8
        jsr add_pages
        lda sum
        sta values+6
        lda sum+1
        sta values+7
        lda $1234
        sta values+8
        lda $17FF
        sta values+9
        jsr bands_init
        ; the bottom band's zone draws eight bytes of the ROM, a page a line
        lda #<GFXDL
        sta BANDDLL+15*3+2
        lda #>GFXDL
        sta BANDDLL+15*3+1
        ldx #5
gdl:    lda gfx,x
        sta GFXDL,x
        dex
        bpl gdl
        lda #$1A
        sta P0C1
        lda #$4C
        sta P0C2
        lda #$86
        sta P0C3
        jsr wait_vblank
        lda #DMA_ON
        sta CTRL
main:
        jsr wait_vblank
        inc values+15
        jmp main

; Adds X pages from page A into sum.
add_pages:
        sta ptr+1
        lda #0
        sta ptr
        sta sum
        sta sum+1
        ldy #0
add1:   lda (ptr),y
        clc
        adc sum
        sta sum
        bcc add2
        inc sum+1
add2:   iny
        bne add1
        inc ptr+1
        dex
        bne add1
        rts

; One object: eight bytes from $3000 (plus the zone's offset, in pages), palette 0, at x = 40.
gfx:    .byte $00, $18, $30, 40, $00, $00

irq:    rti

        include "bands.inc"

        org $FFFA
        .word bands_nmi, reset, irq
