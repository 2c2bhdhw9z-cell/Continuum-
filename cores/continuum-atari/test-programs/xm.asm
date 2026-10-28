; xm: the XM expansion module's YM2151 FM chip, for comparing with MAME 0.289's. The program
; switches the chip on through the XM's control register, then plays a new setting every 16
; frames: each of the eight ways the four operators can be connected, feedback, the pitch and its
; fraction, detune and multiples, the envelope's four rates and its sustain level, key scaling,
; several channels at once with their own sides of the stereo, the LFO on all four of its
; waveforms with vibrato and tremolo, and the noise on the last operator.
;
; Every write to the chip comes straight after a WSYNC, so it lands on a known clock on both
; emulators, and the two writes of a register (its number, then its value) are 3 cycles apart.
; The TIA and the XM's POKEY stay silent and MARIA's DMA is off.

        processor 6502
        include "hw.inc"

XCTRL   = $0470          ; the XM's control register
YMADDR  = $0460
YMDATA  = $0461

step    = $40
count   = $41
ptr     = $42
tmp     = $44

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
        lda #$84            ; the XM: the FM chip on (and the game in its slot)
        sta WSYNC
        sta XCTRL
main:
        jsr wait_vblank
        dec count
        bne main
        lda #16
        sta count
        ; the next table of writes, if there is one
        ldx step
        cpx #NTABLES
        bcs main
        lda tables_lo,x
        sta ptr
        lda tables_hi,x
        sta ptr+1
        jsr apply
        inc step
        jmp main

; Writes the (register, value) pairs at (ptr), until a register of $FF.
apply:
        ldy #0
apply1:
        lda (ptr),y
        cmp #$FF
        beq applied
        sta tmp
        iny
        lda (ptr),y
        iny
        sty tmp+1
        ldy tmp
        sta WSYNC
        sty YMADDR
        sta YMDATA
        ldy tmp+1
        jmp apply1
applied:
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

; ---- the settings ----------------------------------------------------------------------------
; Each table is (register, value) pairs, ending with $FF. The chip's registers:
;   $01 test (bit 1 holds the LFO), $08 key on/off (bits 3-6 the operators, 0-2 the channel),
;   $0F noise (bit 7 on, bits 0-4 its rate), $14 the timers and CSM, $18 LFO rate,
;   $19 LFO depth (bit 7: 0 tremolo, 1 vibrato), $1B LFO waveform,
;   $20+c side/feedback/connection, $28+c pitch, $30+c pitch fraction, $38+c vibrato/tremolo depth,
;   $40+o detune and multiple, $60+o volume, $80+o key scaling and attack,
;   $A0+o tremolo and decay, $C0+o detune 2 and sustain rate, $E0+o sustain level and release.
; An operator's number is the channel plus 0, 8, 16 or 24 (M1, M2, C1, C2).

; A plain note: connection 7, all four operators heard, each a different volume.
t0:
        .byte $20, $C7, $28, $4A, $30, $00, $38, $00
        .byte $40, $01, $48, $02, $50, $01, $58, $04
        .byte $60, $10, $68, $18, $70, $14, $78, $20
        .byte $80, $1F, $88, $1D, $90, $1F, $98, $1C
        .byte $A0, $0A, $A8, $08, $B0, $0C, $B8, $06
        .byte $C0, $05, $C8, $07, $D0, $04, $D8, $09
        .byte $E0, $2F, $E8, $4D, $F0, $3C, $F8, $5A
        .byte $08, $78, $FF

; Connection 0 (a chain), with the strongest feedback.
t1:
        .byte $08, $00, $20, $C8, $28, $3E, $30, $20
        .byte $40, $04, $48, $01, $50, $02, $58, $08
        .byte $60, $0C, $68, $12, $70, $16, $78, $00
        .byte $80, $1E, $88, $1F, $90, $1A, $98, $1F
        .byte $A0, $07, $A8, $09, $B0, $05, $B8, $0B
        .byte $C0, $86, $C8, $44, $D0, $C3, $D8, $05
        .byte $E0, $1F, $E8, $2E, $F0, $4B, $F8, $18
        .byte $08, $78, $FF

; Three channels at once on connections 1, 4 and 5, each on its own side.
t2:
        .byte $08, $00, $08, $01, $08, $02
        .byte $20, $41, $21, $84, $22, $C5
        .byte $28, $5B, $29, $44, $2A, $62
        .byte $30, $10, $31, $30, $32, $08
        .byte $40, $03, $41, $02, $42, $05
        .byte $48, $01, $49, $04, $4A, $02
        .byte $50, $02, $51, $01, $52, $03
        .byte $58, $01, $59, $02, $5A, $01
        .byte $60, $14, $61, $10, $62, $18
        .byte $68, $10, $69, $14, $6A, $0E
        .byte $70, $16, $71, $0C, $72, $12
        .byte $78, $08, $79, $0A, $7A, $06
        .byte $80, $1F, $81, $1E, $82, $1D
        .byte $88, $1C, $89, $1F, $8A, $1B
        .byte $90, $1F, $91, $1A, $92, $1F
        .byte $98, $1D, $99, $1C, $9A, $1E
        .byte $A0, $08, $A1, $06, $A2, $0A
        .byte $A8, $0C, $A9, $04, $AA, $07
        .byte $B0, $05, $B1, $09, $B2, $03
        .byte $B8, $0B, $B9, $05, $BA, $08
        .byte $C0, $04, $C1, $45, $C2, $86
        .byte $C8, $06, $C9, $03, $CA, $05
        .byte $D0, $08, $D1, $07, $D2, $02
        .byte $D8, $03, $D9, $09, $DA, $04
        .byte $E0, $3C, $E1, $2D, $E2, $4E
        .byte $E8, $5B, $E9, $1F, $EA, $3A
        .byte $F0, $2E, $F1, $4C, $F2, $1D
        .byte $F8, $19, $F9, $3B, $FA, $2C
        .byte $08, $78, $09, $79, $0A, $7A, $FF

; The LFO: a sawtooth, both vibrato and tremolo on, at the deepest setting.
t3:
        .byte $08, $01, $08, $02
        .byte $18, $80, $19, $7F, $19, $FF, $1B, $00
        .byte $38, $73, $A0, $8A, $A8, $88, $B0, $8C, $B8, $86
        .byte $08, $78, $FF

; The LFO's square wave, faster, with only vibrato.
t4:
        .byte $18, $C4, $19, $00, $19, $FF, $1B, $01, $38, $70, $FF

; Its triangle, with only tremolo, and the chip's own hold on the LFO.
t5:
        .byte $18, $20, $19, $7F, $19, $80, $1B, $02, $38, $03
        .byte $01, $02, $01, $00, $FF

; Its noise waveform, at the slowest rate.
t6:
        .byte $18, $00, $19, $6A, $19, $EA, $1B, $03, $38, $52, $FF

; The noise generator on channel 8's last operator, several rates.
t7:
        .byte $1B, $00, $19, $00, $19, $80, $38, $00
        .byte $27, $C7, $2F, $4A, $37, $00, $3F, $00
        .byte $47, $01, $4F, $01, $57, $01, $5F, $01
        .byte $67, $7F, $6F, $7F, $77, $7F, $7F, $08
        .byte $87, $1F, $8F, $1F, $97, $1F, $9F, $1F
        .byte $A7, $00, $AF, $00, $B7, $00, $BF, $04
        .byte $C7, $00, $CF, $00, $D7, $00, $DF, $02
        .byte $E7, $0F, $EF, $0F, $F7, $0F, $FF, $1A
        .byte $0F, $9F, $08, $7F, $FF

; The noise, finer and then at its fastest.
t8:
        .byte $0F, $88, $0F, $80, $FF

; Pitches across the range, with detune 2 and bigger multiples.
t9:
        .byte $0F, $00, $08, $07, $08, $00, $08, $01, $08, $02
        .byte $20, $C7, $28, $14, $30, $7C
        .byte $40, $0F, $48, $0A, $50, $07, $58, $0C
        .byte $C0, $41, $C8, $82, $D0, $C3, $D8, $04
        .byte $60, $0E, $68, $12, $70, $10, $78, $14
        .byte $08, $78, $FF

; The highest pitch, the smallest multiple, key scaling on.
t10:
        .byte $28, $7E, $30, $FC
        .byte $40, $70, $48, $30, $50, $50, $58, $10
        .byte $80, $DF, $88, $9F, $90, $5F, $98, $1F
        .byte $08, $78, $FF

; Keyed off, then one operator keyed on again part-way through the release.
t11:
        .byte $08, $00, $FF
t12:
        .byte $08, $08, $FF

; Every operator silent again.
t13:
        .byte $08, $00
        .byte $60, $7F, $68, $7F, $70, $7F, $78, $7F
        .byte $67, $7F, $6F, $7F, $77, $7F, $7F, $7F
        .byte $FF

NTABLES = 14
tables_lo:
        .byte <t0, <t1, <t2, <t3, <t4, <t5, <t6, <t7, <t8, <t9, <t10, <t11, <t12, <t13
tables_hi:
        .byte >t0, >t1, >t2, >t3, >t4, >t5, >t6, >t7, >t8, >t9, >t10, >t11, >t12, >t13

        org $FFFA
        .word nmi, reset, irq
