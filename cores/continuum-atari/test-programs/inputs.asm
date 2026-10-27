; inputs: the controls as the 7800 reads them. Each frame, in VBLANK, it reads the joysticks
; (SWCHA), the console switches (SWCHB) and the six TIA inputs (INPT0-5) in two-button mode (port B
; bits 2 and 4 driven low), then again in one-button mode (port B all inputs), and shows the sixteen
; bytes as bands (bands.inc). The comparison presses each stick direction, button and switch on
; MAME and here on the same frames.

        processor 6502
        include "hw.inc"

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
        sta $2000,x
        sta $2100,x
        sta $2200,x
        inx
        bne clr
        jsr bands_init
        jsr wait_vblank
        lda #DMA_ON
        sta CTRL
main:
        jsr wait_vblank
        ; two-button mode: port B bits 2 and 4 as outputs, driven low
        lda #$14
        sta CTLSWB
        lda #$00
        sta SWCHB
        lda SWCHA
        sta values+0
        lda SWCHB
        sta values+1
        lda INPT0
        sta values+2
        lda INPT1
        sta values+3
        lda INPT2
        sta values+4
        lda INPT3
        sta values+5
        lda INPT4
        sta values+6
        lda INPT5
        sta values+7
        ; one-button mode: port B all inputs (pulled high)
        lda #$00
        sta CTLSWB
        lda SWCHA
        sta values+8
        lda SWCHB
        sta values+9
        lda INPT0
        sta values+10
        lda INPT1
        sta values+11
        lda INPT2
        sta values+12
        lda INPT3
        sta values+13
        lda INPT4
        sta values+14
        lda INPT5
        sta values+15
        jmp main

irq:    rti

        include "bands.inc"

        org $FFFA
        .word bands_nmi, reset, irq
