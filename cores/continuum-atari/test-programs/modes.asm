; modes: every way MARIA draws. Sixteen zones, and at the end of each a display list interrupt
; sets the next zone's control register (read mode, character width, kangaroo mode, colour kill)
; and background: 160A with all eight palettes; 160B through a 5-byte header (write mode kept by
; the 4-byte headers after it); 320A, 320B, 320C and 320D; kangaroo mode; character mode with
; one-byte and two-byte characters; holey DMA over both kinds of hole; objects that overlap,
; that sit at the right edge and that wrap round to the left; colour kill. The pictures are
; compared with MAME's.

        processor 6502
        include "hw.inc"

DLL     = $1800
DLs     = $1900             ; zone n's list at $1900 + $20n
zone    = $40
frame   = $41

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
        sta $1A00,x
        sta $1B00,x
        sta $2000,x
        sta $2100,x
        sta $2200,x
        inx
        bne clr
        ; the 24 colours: palette p, colour c = $10 * (p * 3 + c) + 6 or so, all different
        ldx #0
pal:    lda colours,x
        sta P0C1,x
        inx
        cpx #31
        bne pal
        lda #$00
        sta BACKGRND
        lda #>CHARS
        sta CHARBASE
        ; the display list list: 16 zones of 14 lines, then 18 lines, each with an interrupt
        ldx #0
        ldy #0
mk:     lda holes,y
        ora #$8D            ; DLI, 14 lines, and the zone's holey bits
        cpy #15
        bne notlast
        and #$F0
        ora #$81            ; the last: 2 lines
notlast:
        sta DLL,x
        tya
        lsr
        lsr
        lsr
        clc
        adc #>DLs
        sta DLL+1,x
        tya
        and #7
        asl
        asl
        asl
        asl
        asl
        sta DLL+2,x
        inx
        inx
        inx
        iny
        cpy #16
        bne mk
        ; copy each zone's display list from the table
        ldx #0
fill:   stx zone
        txa
        asl
        tay
        lda lists,y
        sta $80
        lda lists+1,y
        sta $81
        txa
        lsr
        lsr
        lsr
        clc
        adc #>DLs
        sta $83
        txa
        and #7
        asl
        asl
        asl
        asl
        asl
        sta $82
        ldy #0
cp:     lda ($80),y
        sta ($82),y
        iny
        cpy #32
        bne cp
        ldx zone
        inx
        cpx #16
        bne fill
        lda #>DLL
        sta DPPH
        lda #<DLL
        sta DPPL
vb0:    bit MSTAT
        bpl vb0
        lda ctrls
        sta CTRL
main:
vb1:    bit MSTAT
        bmi vb1
vb2:    bit MSTAT
        bpl vb2
        lda #0
        sta zone
        lda ctrls
        sta CTRL
        lda bgs
        sta BACKGRND
        inc frame
        ; the first zone's first object moves, so frames differ
        lda frame
        and #$3F
        sta DLs+3
        jmp main

nmi:
        pha
        txa
        pha
        inc zone
        ldx zone
        lda ctrls,x
        sta WSYNC
        sta CTRL
        lda bgs,x
        sta BACKGRND
        pla
        tax
        pla
        rti

irq:    rti

; per zone: the control register (DMA on always) and the background
ctrls:  .byte $40, $40, $40, $43, $42, $43, $42, $44, $40, $50, $40, $40, $40, $C0, $47, $46
bgs:    .byte $00, $02, $04, $06, $08, $0A, $0C, $0E, $10, $20, $30, $40, $50, $60, $70, $80
; per zone: holey DMA bits for its DLL entry (bit 6: 4K holes over $9000, bit 5: 2K over $8800)
holes:  .byte 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, $40, $20, 0, 0, 0, 0
colours:
        .byte $14, $26, $38, $00, $44, $56, $68, $00, $74, $86, $98, $00, $A4, $B6, $C8, $00
        .byte $D4, $E6, $F8, $00, $1C, $2E, $3A, $00, $4C, $5E, $6A, $00, $7C, $8E, $9A

lists:  .word l0, l1, l2, l3, l4, l5, l6, l7, l8, l9, l10, l11, l12, l13, l14, l15
; 160A: an object in each of the eight palettes (width 2 bytes = 8 dots)
l0:     .byte <G, $1E, >G, 0, <G, $3E, >G, 10, <G, $5E, >G, 20, <G, $7E, >G, 30
        .byte <G, $9E, >G, 40, <G, $BE, >G, 50, <G, $DE, >G, 60, <G, $FE, >G, 70
; 160B through a 5-byte header, then 4-byte headers keep write mode 1 (palettes 0 and 4)
l1:     .byte <G, $C0, >G, $1C, 10, <G, $1C, >G, 60, <G, $9C, >G, 90, 0, 0
; 160A overlapping objects: later ones over earlier ones where not transparent
l2:     .byte <G, $18, >G, 20, <G, $38, >G, 24, <G, $58, >G, 28, 0, 0
; 320A (read mode 3, 160A data): two objects
l3:     .byte <G, $18, >G, 10, <G, $38, >G, 80, 0, 0
; 320B (read mode 2) with write mode 1 data
l4:     .byte <G, $C0, >G, $18, 10, <G, $C0, >G, $58, 80, 0, 0
; 320C (read mode 3) with write mode 1 data
l5:     .byte <G, $C0, >G, $18, 10, <G, $C0, >G, $B8, 80, 0, 0
; 320D (read mode 2) with write mode 0 data
l6:     .byte <G, $40, >G, $18, 10, <G, $40, >G, $78, 80, 0, 0
; kangaroo mode: colour 0 is drawn, not transparent
l7:     .byte <G, $18, >G, 20, <G, $38, >G, 24, 0, 0
; character mode, one byte a character (a 5-byte header with bit 5)
l8:     .byte <M, $60, >M, $10, 10, 0, 0
; character mode, two bytes a character (the control register's bit 4)
l9:     .byte <M, $60, >M, $18, 10, 0, 0
; holey DMA over 4K holes: graphics at $9000 are holes on odd pages of the zone
l10:    .byte <H1, $18, >H1, 20, 0, 0
; holey DMA over 2K holes
l11:    .byte <H2, $18, >H2, 20, 0, 0
; at the right edge, and wrapping round to the left
l12:    .byte <G, $18, >G, 150, <G, $38, >G, 250, 0, 0
; colour kill (the control register's bit 7): the same objects in grey
l13:    .byte <G, $1E, >G, 0, <G, $3E, >G, 10, <G, $5E, >G, 20, <G, $7E, >G, 30, 0, 0
; kangaroo with 160B
l14:    .byte <G, $C0, >G, $1C, 10, <G, $9C, >G, 40, 0, 0
; 320 with kangaroo mode
l15:    .byte <G, $18, >G, 20, <G, $38, >G, 60, 0, 0

        ; holey graphics, 4K holes: the zone reads pages $8F to $9C; $90-$9F are holes, so only
        ; the zone's last line (page $8F) draws. The holes hold graphics that must not show.
        org $8F00
H1:
        REPEAT 16
        .byte $FF, $AA, $55, $E4, $1B, $C3, $3C, $99
        DS.B 248, $66
        REPEND
        ; holey graphics, 2K holes: pages $A0 to $AD; $A8-$AF are holes
        org $A000
H2:
        REPEAT 16
        .byte $E4, $FF, $1B, $AA, $C3, $55, $99, $3C
        DS.B 248, $5A
        REPEND
        ; graphics: 16 pages from $C000 (a zone's lines read page + offset), each different
        org $C000
G:
        REPEAT 16
        .byte $E4, $1B, $FF, $AA, $55, $E4, $1B, $00, $C3, $3C, $99, $66, $F0, $0F, $81, $7E
        .byte $27, $D8, $11, $EE, $5A, $A5, $0F, $F0, $33, $CC, $96, $69, $01, $80, $7F, $FE
        DS.B 224, $3C
        REPEND
        ; the characters' graphics: 16 pages from $D000
        org $D000
CHARS:
        REPEAT 16
        .byte $A5, $5A, $C3, $3C, $0F, $F0, $99, $66, $E1, $1E, $B4, $4B, $D2, $2D, $78, $87
        DS.B 240, $81
        REPEND
        org $E000
M:      .byte 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15

        org $FFFA
        .word nmi, reset, irq
