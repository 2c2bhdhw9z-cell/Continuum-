; timer: the RIOT's interval timer, read at known moments in VBLANK (when MARIA's DMA is idle):
; each prescale, a read straight after the write, reads before and after the count runs out, the
; interrupt flag, and the count going on once a cycle after it runs out. Each frame's 16 readings
; are shown as bands (bands.inc). Proves the RIOT against MAME.

        processor 6502
        include "hw.inc"

frame   = $40

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
        inc frame
        include "timer_body.inc"
        lda frame
        sta values+14
        lda #$0F
        sta values+15
        jmp main

irq:    rti

        include "bands.inc"

        org $FFFA
        .word bands_nmi, reset, irq
