; timer2600: the same RIOT checks as timer.asm (timer_body.inc), on an Atari 2600 cartridge, so
; Stella, whose 6532 is measured against real consoles, can say what the chip reads. The results
; stay in RAM at $80-$8D; a plain frame loop keeps the console running.

        processor 6502
        include "hw.inc"

VSYNC   = $00
VBLANK  = $01
values  = $80

        org $F000
reset:
        sei
        cld
        ldx #$FF
        txs
        lda #0
clr:    sta $00,x
        dex
        bne clr
        include "timer_body.inc"
frame:  lda #2
        sta WSYNC2
        sta VSYNC
        sta WSYNC2
        sta WSYNC2
        sta WSYNC2
        lda #0
        sta VSYNC
        ldx #252
lines:  sta WSYNC2
        dex
        bne lines
        jmp frame

WSYNC2  = $02

        org $FFFA
        .word reset, reset, reset
