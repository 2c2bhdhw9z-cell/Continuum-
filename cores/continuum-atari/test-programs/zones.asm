; zones: MARIA's display list list and display lists, 160A objects that move every frame, a
; display list interrupt that changes the background part-way down, and the frame counted by
; MSTAT's VBLANK. Proves the zones, the DMA, the DLI's timing and the colours against MAME.

        processor 6502
        include "hw.inc"

DLL     = $1800             ; 16 zones x 3 bytes
DL0     = $1900             ; the moving objects' list
DLE     = $1980             ; an empty list
frame   = $40
xpos    = $41
dlicnt  = $42

        org $8000
reset:
        sei
        cld
        ldx #$FF
        txs
        lda #DMA_OFF
        sta CTRL
        ; clear RAM $1800-$27FF
        lda #0
        tax
clr:    sta $1800,x
        sta $1900,x
        sta $1A00,x
        sta $1B00,x
        sta $1C00,x
        sta $1D00,x
        sta $1E00,x
        sta $1F00,x
        sta $2000,x
        sta $2100,x
        sta $2200,x
        inx
        bne clr
        ; colours
        lda #$00
        sta BACKGRND
        lda #$1A
        sta P0C1
        lda #$46
        sta P0C2
        lda #$88
        sta P0C3
        lda #$CA
        sta P1C1
        lda #$3E
        sta P1C2
        lda #$F4
        sta P1C3
        ; the display list list: 15 zones of 16 lines, then one of 2 (242 lines in all)
        ldx #0
        ldy #0
dll:    lda #$0F
        cpy #5
        bne nodli
        ora #$80            ; zone 5 asks for an interrupt (it comes at the end of zone 4)
nodli:  cpy #10
        bne nodli2
        ora #$80
nodli2: sta DLL,x
        inx
        tya
        and #1
        bne empty
        lda #>DL0
        sta DLL,x
        inx
        lda #<DL0
        sta DLL,x
        inx
        jmp next
empty:  lda #>DLE
        sta DLL,x
        inx
        lda #<DLE
        sta DLL,x
        inx
next:   iny
        cpy #15
        bne dll
        lda #$01            ; the last zone: 2 lines
        sta DLL,x
        lda #>DLE
        sta DLL+1,x
        lda #<DLE
        sta DLL+2,x
        ; the moving objects: two 4-byte headers and the end
        lda #<gfx
        sta DL0
        lda #$18            ; palette 0, width 8 bytes (32 pixels)
        sta DL0+1
        lda #>gfx
        sta DL0+2
        lda #16
        sta DL0+3
        lda #<gfx
        sta DL0+4
        lda #$3C            ; palette 1, width 4 bytes
        sta DL0+5
        lda #>gfx
        sta DL0+6
        lda #100
        sta DL0+7
        ; DLE and DL0+8 stay zero: the end of a list
        lda #>DLL
        sta DPPH
        lda #<DLL
        sta DPPL
        ; wait for VBLANK, then turn DMA on
wv1:    bit MSTAT
        bpl wv1
        lda #DMA_ON
        sta CTRL
main:
        ; wait for the end of VBLANK, then for the next VBLANK
wv2:    bit MSTAT
        bmi wv2
wv3:    bit MSTAT
        bpl wv3
        inc frame
        lda frame
        and #$7F
        sta DL0+3           ; the first object moves one cell a frame
        eor #$7F
        sta DL0+7           ; the second the other way
        lda #0
        sta dlicnt
        jmp main

nmi:
        pha
        inc dlicnt
        lda dlicnt
        asl
        asl
        asl
        asl
        ora #$04
        sta WSYNC
        sta BACKGRND
        pla
        rti

irq:
        rti

        ; 16 pages of graphics, $C000-$CFFF: page n's bytes show stripes that differ by line
        org $C000
gfx:
        REPEAT 16
        .byte $E4, $1B, $FF, $AA, $55, $E4, $1B, $00, $C3, $3C, $99, $66, $F0, $0F, $81, $7E
        DS.B 240, $00
        REPEND

        org $FFFA
        .word nmi, reset, irq
