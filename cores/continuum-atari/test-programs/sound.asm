; sound: the TIA's two channels through every AUDC mode, several AUDF frequencies and volumes,
; changed on chosen frames (in VBLANK) and once more part-way through a line (after a WSYNC), so
; the sound can be compared with MAME's sample for sample. Channel 1 plays a different mode from
; channel 0 at the same time.

        processor 6502
        include "hw.inc"

step    = $40
count   = $41

        org $8000
reset:
        sei
        cld
        ldx #$FF
        txs
        lda #DMA_OFF
        sta CTRL
        lda #0
        sta AUDV0
        sta AUDV1
        sta step
        lda #4
        sta count
main:
vb1:    bit MSTAT
        bmi vb1
vb2:    bit MSTAT
        bpl vb2
        dec count
        bne main
        lda #4              ; a new setting every four frames
        sta count
        ldx step
        lda table,x
        cmp #$FF
        beq quiet
        sta AUDC0
        lda table+1,x
        sta AUDF0
        lda table+2,x
        sta AUDV0
        lda table+3,x
        sta AUDC1
        lda table+4,x
        sta AUDF1
        ; channel 1's volume changes part-way through a line
        sta WSYNC
        ldy #6
w:      dey
        bne w
        lda table+5,x
        sta AUDV1
        txa
        clc
        adc #6
        sta step
        jmp main
quiet:  lda #0
        sta AUDV0
        sta AUDV1
        jmp main

nmi:    rti
irq:    rti

; AUDC0, AUDF0, AUDV0, AUDC1, AUDF1, AUDV1
table:
        .byte $00, $00, $0F, $00, $00, $00
        .byte $01, $05, $0F, $0C, $1F, $08
        .byte $02, $0A, $0C, $0D, $03, $0A
        .byte $03, $00, $0A, $0E, $11, $0F
        .byte $04, $1F, $0F, $0F, $07, $06
        .byte $05, $0E, $08, $01, $1A, $0C
        .byte $06, $13, $0F, $02, $00, $0F
        .byte $07, $01, $0B, $03, $09, $07
        .byte $08, $00, $0F, $04, $17, $0B
        .byte $08, $1F, $07, $05, $0D, $0F
        .byte $09, $04, $0F, $06, $02, $09
        .byte $0A, $10, $0D, $07, $1C, $0F
        .byte $0B, $08, $0F, $08, $05, $05
        .byte $0C, $02, $0F, $09, $14, $0E
        .byte $0D, $1B, $09, $0A, $0B, $0F
        .byte $0E, $06, $0F, $0B, $1E, $03
        .byte $0F, $0C, $0F, $00, $00, $0F
        .byte $0F, $1D, $0A, $0F, $06, $0C
        .byte $04, $00, $0F, $04, $01, $0F
        .byte $0C, $00, $0F, $0C, $1F, $0F
        .byte $FF

        org $FFFA
        .word nmi, reset, irq
