; dma: how long MARIA's DMA holds the processor. Zones carry different loads (no objects, one,
; four, sixteen; 4-byte and 5-byte headers; character-mode objects of one and two bytes a character;
; holey DMA; and a zone heavy enough to run out of time), and the processor counts how many times a
; ten-cycle loop runs from the end of one VBLANK to the start of the next. The counts, low and high
; byte, go into the bands (bands.inc), along with the counts for each half of the picture. A cycle
; more or less of DMA anywhere moves them. Proves the DMA timing against MAME.

        processor 6502
        include "hw.inc"

DLL     = $1800
DLs     = $1900             ; the zones' display lists, $40 bytes each
count   = $40               ; 3 bytes
half    = $43
frame   = $46
zone    = $47

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
        sta $1C00,x
        sta $1D00,x
        sta $1E00,x
        sta $1F00,x
        sta $2000,x
        sta $2100,x
        sta $2200,x
        inx
        bne clr
        lda #$0E
        sta P0C1
        lda #$46
        sta P0C2
        lda #$88
        sta P0C3
        lda #$C8
        sta P1C1
        lda #CHARS/256
        sta CHARBASE
        ; the display list list: 16 zones of 15 lines and one of 2, each zone its own list
        ldx #0
        ldy #0
mkdll:  lda #$0E
        sta DLL,x
        tya
        clc
        adc #>DLs           ; zone n's list at $1900 + $40n: high byte $19 + n/4
        lsr
        lsr
        lda #0
        sta DLL+1,x         ; filled below
        sta DLL+2,x
        inx
        inx
        inx
        iny
        cpy #17
        bne mkdll
        lda #$01
        sta DLL+48
        ; each zone's list address
        ldx #0
        ldy #0
mkptr:  tya
        lsr
        lsr
        clc
        adc #>DLs
        sta DLL+1,x
        tya
        and #3
        asl
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
        cpy #17
        bne mkptr
        ; fill the lists from the table: for each zone, its headers, then two zero bytes
        ldx #0              ; zone
fill:   stx zone
        txa
        asl
        tay
        lda loads,y
        sta $80
        lda loads+1,y
        sta $81
        lda DLL+2,x
        ; (DLL+3x is the zone's entry; recompute its list address)
        txa
        lsr
        lsr
        clc
        adc #>DLs
        sta $83
        txa
        and #3
        asl
        asl
        asl
        asl
        asl
        asl
        sta $82
        ldy #0
copy:   lda ($80),y
        cmp #$FF
        beq done1
        sta ($82),y
        iny
        jmp copy
done1:  lda #0
        sta ($82),y
        iny
        sta ($82),y
        ldx zone
        inx
        cpx #17
        bne fill
        lda #>DLL
        sta DPPH
        lda #<DLL
        sta DPPL
vb0:    bit MSTAT
        bpl vb0
        lda #DMA_ON
        sta CTRL

main:
        ; count from the end of VBLANK to the start of the next one
vb1:    bit MSTAT
        bmi vb1
        lda #0
        sta count
        sta count+1
        sta count+2
loop:   inc count           ; 5 cycles
        bne nocarry         ; 3 when taken
        inc count+1
nocarry:
        bit MSTAT           ; 4
        bpl loop            ; 3
        ; DMA is off now until the next frame's line 16: put the counts on the colour registers
        lda count
        sta P2C1
        lda count+1
        sta P2C2
        inc frame
        lda frame
        sta P2C3
        jmp main

irq:    rti
nmi:    rti

; each zone's objects: headers then $FF
loads:
        .word z0, z1, z2, z3, z4, z5, z6, z7, z8, z9, z10, z11, z12, z13, z14, z15, z16
z0:     .byte $FF                                   ; nothing
z1:     .byte <GFX, $1F, >GFX, 10, $FF              ; one 1-byte object
z2:     .byte <GFX, $1C, >GFX, 10, <GFX, $1C, >GFX, 40, <GFX, $1C, >GFX, 70, <GFX, $1C, >GFX, 100, $FF ; four 4-byte objects
z3:     .byte <GFX, $40, >GFX, $1C, 20, $FF         ; one 5-byte header, write mode 0
z4:     .byte <GFX, $C0, >GFX, $1C, 20, <GFX, $1E, >GFX, 60, $FF ; 160B, then a 4-byte header in it
z5:     .byte <MAP, $60, >MAP, $1C, 30, $FF         ; character mode, 4 characters
z6:     .byte <GFX, $10, >GFX, 5, $FF               ; one 16-byte-wide object
z7:     .byte <GFX, $10, >GFX, 0, <GFX, $10, >GFX, 64, <GFX, $10, >GFX, 128, <GFX, $10, >GFX, 192, $FF
z8:     .byte <GFX, $40, >GFX, $00, 0, <GFX, $40, >GFX, $00, 128, $FF ; two 32-byte objects
z9:     .byte <GFX, $40, >GFX, $00, 0, <GFX, $40, >GFX, $00, 64, <GFX, $40, >GFX, $00, 128, <GFX, $40, >GFX, $00, 192, $FF ; out of time
z10:    .byte <HGFX, $1C, >HGFX, 10, $FF            ; holey: never in a holey area here (holey off)
z11:    .byte <MAP, $60, >MAP, $18, 50, $FF         ; character mode, 8 characters
z12:    .byte <GFX, $3E, >GFX, 90, <GFX, $5E, >GFX, 91, <GFX, $7E, >GFX, 92, $FF
z13:    .byte $FF
z14:    .byte <GFX, $1F, >GFX, 159, $FF             ; at the right edge
z15:    .byte <GFX, $18, >GFX, 250, $FF             ; wraps round the left edge
z16:    .byte $FF

        org $C000
GFX:
        REPEAT 16
        .byte $E4, $1B, $FF, $AA, $55, $E4, $1B, $00, $C3, $3C, $99, $66, $F0, $0F, $81, $7E
        .byte $11, $22, $33, $44, $55, $66, $77, $88, $99, $AA, $BB, $CC, $DD, $EE, $FF, $01
        DS.B 224, $3C
        REPEND
        org $D000
CHARS:
        REPEAT 16
        .byte $A5, $5A, $C3, $3C, $0F, $F0, $99, $66
        DS.B 248, $81
        REPEND
        org $E000
MAP:    .byte 0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7
HGFX = $C000

        org $FFFA
        .word nmi, reset, irq
