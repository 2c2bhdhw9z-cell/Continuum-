; pokey: POKEY's sound, for comparing with MAME 0.289's. A cartridge with a POKEY at $4000 (the
; board Ballblazer and Commando use) plays a new setting every eight frames: the four channels with
; each distortion, the 17 and 9-bit noise, the 15 kHz and 1.79 MHz clocks, channels joined into 16
; bits, the high-pass filters, settings changed without restarting the counters, volume-only output
; with a volume written every line, the counters stopped and restarted by SKCTL, two-tone mode both
; ways, and asynchronous serial mode holding channels 3 and 4. Every write to POKEY comes straight
; after a WSYNC, with an absolute store, so it lands on a known clock on both emulators. The TIA is
; silent and MARIA's DMA is off. The four volumes never add up to more than 44, the most MAME's
; POKEY puts out before it clips.

        processor 6502
        include "hw.inc"

; POKEY's registers. P for POKEY: hw.inc has the TIA's AUDC1 and AUDF1.
PAUDF1  = $4000
PAUDC1  = $4001
PAUDF2  = $4002
PAUDC2  = $4003
PAUDF3  = $4004
PAUDC3  = $4005
PAUDF4  = $4006
PAUDC4  = $4007
PAUDCTL = $4008
PSTIMER = $4009
PSKCTL  = $400F

step    = $40
count   = $41
tmp     = $42
SETLEN  = 10
NSETS   = 16

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
        lda #1
        sta count
        jsr wait_vblank
        lda #3
        sta WSYNC
        sta PSKCTL          ; POKEY's counters start
main:
        jsr wait_vblank
        dec count
        bne main
        lda #8
        sta count
        lda step
        cmp #NSETS
        bcs extras
        asl                 ; x10: the setting's place in the table
        sta tmp
        asl
        asl
        clc
        adc tmp
        tax
        jsr apply
next:   inc step
        jmp main

; The steps after the table, one routine each.
extras:
        sec
        sbc #NSETS
        cmp #(extab_end-extab)/2
        bcs idle
        asl
        tax
        lda extab+1,x
        pha
        lda extab,x
        pha
        rts
idle:   jmp main

extab:  .word ex_ramp-1, ex_noise-1, ex_stop-1, ex_two-1, ex_two2-1, ex_async-1, ex_sync-1, ex_quiet-1
extab_end:

; Channel 1 volume-only, a new volume every line for 200 lines; the others quiet.
ex_ramp:
        lda #$10
        sta WSYNC
        sta PAUDC2
        sta WSYNC
        sta PAUDC3
        sta WSYNC
        sta PAUDC4
        ldy #0
ramp1:  lda ramptab,y
        sta WSYNC
        sta PAUDC1
        iny
        cpy #200
        bne ramp1
        jmp next

; A noise, then (four frames on) SKCTL stops the counters for 40 lines and starts them again.
ex_noise:
        ldx #NSETS*SETLEN
        jsr apply
        lda #4
        sta count
        jmp next
ex_stop:
        lda #0
        sta WSYNC
        sta PSKCTL          ; stop: the counters and polynomials hold
        ldy #40
stop1:  sta WSYNC
        dey
        bne stop1
        lda #3
        sta WSYNC
        sta PSKCTL          ; and start again from the beginning
        jmp next

; Two tones in two-tone mode: with the break bit, only channel 2 restarts channel 1; without it,
; each restarts the other.
ex_two:
        ldx #(NSETS+1)*SETLEN
        jsr apply
        lda #$8B
        sta WSYNC
        sta PSKCTL
        jmp next
ex_two2:
        lda #$0B
        sta WSYNC
        sta PSKCTL
        jmp next

; Asynchronous serial mode: channels 3 and 4 stop (and restart) while channel 1 plays on.
ex_async:
        ldx #(NSETS+2)*SETLEN
        jsr apply
        lda #$13
        sta WSYNC
        sta PSKCTL
        jmp next
ex_sync:
        lda #$03
        sta WSYNC
        sta PSKCTL
        jmp next

ex_quiet:
        ldx #(NSETS+3)*SETLEN
        jsr apply
        jmp next

; Writes the setting at snd+X: AUDCTL, AUDF1, AUDC1 ... AUDC4, then STIMER unless its byte is 0.
apply:
        lda snd+0,x
        sta WSYNC
        sta PAUDCTL
        lda snd+1,x
        sta WSYNC
        sta PAUDF1
        lda snd+2,x
        sta WSYNC
        sta PAUDC1
        lda snd+3,x
        sta WSYNC
        sta PAUDF2
        lda snd+4,x
        sta WSYNC
        sta PAUDC2
        lda snd+5,x
        sta WSYNC
        sta PAUDF3
        lda snd+6,x
        sta WSYNC
        sta PAUDC3
        lda snd+7,x
        sta WSYNC
        sta PAUDF4
        lda snd+8,x
        sta WSYNC
        sta PAUDC4
        lda snd+9,x
        beq nostimer
        sta WSYNC
        sta PSTIMER
nostimer:
        rts

wait_vblank:
        bit MSTAT
        bmi wait_vblank
wait_vb2:
        bit MSTAT
        bpl wait_vb2
        rts

nmi:    rti
irq:    rti

; AUDCTL, AUDF1, AUDC1, AUDF2, AUDC2, AUDF3, AUDC3, AUDF4, AUDC4, STIMER (0: none)
snd:
        .byte $00, $40, $AB, $50, $A9, $60, $A8, $70, $A6, 1   ; four pure tones, 64 kHz
        .byte $00, $20, $0B, $30, $2A, $40, $4A, $50, $69, 1   ; poly5+17, poly5, poly5+4, poly5 again
        .byte $00, $10, $8B, $18, $CB, $07, $EA, $00, $00, 1   ; 17-bit noise, poly4, pure (bit 6 set)
        .byte $80, $10, $8B, $1C, $0B, $00, $00, $00, $00, 1   ; 9-bit noise, poly5+9
        .byte $80, $03, $8B, $00, $00, $00, $00, $00, $00, 1   ; 9-bit noise, fast
        .byte $01, $10, $AB, $18, $AA, $05, $A9, $02, $88, 1   ; 15 kHz clock
        .byte $40, $80, $AB, $40, $AA, $00, $00, $00, $00, 1   ; channel 1 at 1.79 MHz
        .byte $20, $33, $AA, $00, $00, $90, $AB, $30, $8A, 1   ; channel 3 at 1.79 MHz
        .byte $50, $80, $A0, $02, $AF, $33, $A8, $00, $00, 1   ; channels 1+2 joined, 1.79 MHz
        .byte $28, $45, $AA, $00, $00, $20, $A0, $01, $AE, 1   ; channels 3+4 joined, 1.79 MHz
        .byte $10, $10, $A0, $01, $AE, $08, $8A, $00, $00, 1   ; channels 1+2 joined, 64 kHz
        .byte $04, $40, $AE, $60, $AA, $41, $A0, $00, $00, 1   ; high-pass on channel 1 by channel 3
        .byte $02, $22, $8A, $50, $AE, $00, $00, $51, $A0, 1   ; high-pass on channel 2 by channel 4
        .byte $07, $08, $AC, $0A, $AC, $09, $A5, $0B, $A5, 1   ; both high-pass filters, 15 kHz
        .byte $00, $47, $AB, $5B, $A9, $6F, $A8, $83, $A6, 0   ; new pitches, counters not restarted
        .byte $00, $00, $1B, $00, $18, $00, $14, $00, $11, 0   ; volume only
        .byte $00, $05, $8B, $09, $4A, $00, $00, $00, $00, 1   ; noise for the SKCTL test
        .byte $00, $30, $AA, $47, $A9, $00, $A0, $00, $A0, 1   ; two tones for two-tone mode
        .byte $00, $35, $A9, $00, $A0, $21, $AA, $2C, $A8, 1   ; tones on 1, 3 and 4 for async mode
        .byte $00, $00, $A0, $00, $A0, $00, $A0, $00, $A0, 1   ; silence

ramptab:
        .byte $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $1A, $1B, $1C, $1D, $1E, $1F
        .byte $1F, $1E, $1D, $1C, $1B, $1A, $19, $18, $17, $16, $15, $14, $13, $12, $11, $10
        .byte $18, $10, $18, $10, $1F, $10, $1F, $10, $15, $1A, $15, $1A, $13, $1C, $13, $1C
        .byte $10, $10, $1F, $1F, $10, $10, $1F, $1F, $17, $17, $17, $18, $18, $18, $10, $1F
        .byte $12, $14, $16, $18, $1A, $1C, $1E, $1D, $1B, $19, $17, $15, $13, $11, $10, $10
        .byte $1F, $10, $1E, $11, $1D, $12, $1C, $13, $1B, $14, $1A, $15, $19, $16, $18, $17
        .byte $10, $13, $16, $19, $1C, $1F, $1C, $19, $16, $13, $10, $13, $16, $19, $1C, $1F
        .byte $11, $11, $11, $1E, $1E, $1E, $12, $12, $1D, $1D, $13, $1C, $14, $1B, $15, $1A
        .byte $10, $1F, $10, $1F, $10, $1F, $10, $1F, $18, $18, $18, $18, $10, $10, $10, $10
        .byte $1A, $1B, $1C, $1D, $1E, $1F, $1E, $1D, $1C, $1B, $1A, $19, $18, $17, $16, $15
        .byte $14, $13, $12, $11, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $1A, $1B
        .byte $1C, $1D, $1E, $1F, $10, $1F, $10, $1F, $10, $1F, $10, $1F, $15, $15, $15, $15
        .byte $10, $10, $10, $10, $10, $10, $10, $10

        org $FFFA
        .word nmi, reset, irq
