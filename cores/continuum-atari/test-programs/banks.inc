; banks.inc: the bank-switching checks, for one board picked by MAPPER (1 SuperGame, 2 SuperGame
; with nine banks, 3 SuperGame with RAM at $4000, 4 Absolute, 5 Activision). The rest of the
; cartridge is filler the test builds (tests/common/banks.rs and tests/mame/make_refs.py): each
; 16 KB bank k starts with $10+k, $80+k, then k. Every frame the program switches banks and shows
; what each window reads, as bands (bands.inc).

        processor 6502
        include "hw.inc"

        org CODE
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
    IF MAPPER <= 3
        ; SuperGame: a write to $8000-$BFFF picks the bank shown there
        ldx #0
sg:     txa
        sta $8000
        lda $8000
        sta values,x
        inx
        cpx #8
        bne sg
        lda #3
        sta $A123           ; any address in the window switches
        lda $8001
        sta values+8
        lda $4000           ; bank 6 (or bank 0 with nine banks; RAM on the RAM board)
        sta values+9
        lda $C000+[sig-CODE]  ; the fixed bank's own byte
        sta values+10
      IF MAPPER == 3
        lda #$5A
        sta $4000
        lda #$A5
        sta $7FFF
        lda $4000
        sta values+11
        lda $7FFF
        sta values+12
      ENDIF
    ENDIF
    IF MAPPER == 4
        ; Absolute: a write of 1 or 2 to $8000 picks which 16 KB bank shows at $4000
        lda #1
        sta $8000
        lda $4000
        sta values+0
        lda $4001
        sta values+1
        lda #2
        sta $8000
        lda $4000
        sta values+2
        lda $4001
        sta values+3
        lda #0              ; neither bit: no change
        sta $8000
        lda $4000
        sta values+4
        lda $8000           ; the top half is fixed: banks 2 and 3
        sta values+5
        lda $C000
        sta values+6
    ENDIF
    IF MAPPER == 5
        ; Activision: a write to $E000+ picks, by its address's low three bits, the bank at $A000
        ldx #0
act:    sta $FF80,x
        lda $A000
        sta values,x
        inx
        cpx #8
        bne act
        lda $4000           ; bank 6 at $4000-$7FFF
        sta values+8
        lda $8000           ; the first half of bank 7 at $8000
        sta values+9
        lda $A001
        sta values+10
    ENDIF
        lda #$0F
        sta values+15
        jmp main

irq:    rti
sig:    .byte $E7

        include "bands.inc"

        org CODE_END-6
        .word bands_nmi, reset, irq
