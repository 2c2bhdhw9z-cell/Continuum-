; bands.inc: shows 16 bytes as 16 coloured bands down the screen, one per zone, so a test
; program's measurements are in the picture where MAME and cAtari can be compared. Each zone of the
; display list list asks for a display list interrupt; the handler sets the background to the next
; byte. Include it after the program's code; call bands_init with DMA off, and wait_vblank once a
; frame (it resets the band counter at the start of VBLANK).

values  = $50               ; the 16 bytes shown
bandidx = $60
BANDDLL = $1800             ; 17 zones x 3 bytes
BANDDL  = $1880             ; an empty display list

bands_init:
        ldx #0
        ldy #0
bands_zone:
        lda #$8E            ; display list interrupt, 15 lines
        sta BANDDLL,x
        lda #>BANDDL
        sta BANDDLL+1,x
        lda #<BANDDL
        sta BANDDLL+2,x
        inx
        inx
        inx
        iny
        cpy #16
        bne bands_zone
        lda #$01            ; the last zone: 2 lines (242 in all)
        sta BANDDLL,x
        lda #>BANDDL
        sta BANDDLL+1,x
        lda #<BANDDL
        sta BANDDLL+2,x
        lda #0
        sta BANDDL
        sta BANDDL+1
        lda #>BANDDLL
        sta DPPH
        lda #<BANDDLL
        sta DPPL
        rts

bands_nmi:
        pha
        txa
        pha
        ldx bandidx
        lda values,x
        sta BACKGRND
        inc bandidx
        pla
        tax
        pla
        rti

; returns at the start of VBLANK, with the band counter reset
wait_vblank:
        bit MSTAT
        bmi wait_vblank
wait_vb2:
        bit MSTAT
        bpl wait_vb2
        lda #0
        sta bandidx
        rts
