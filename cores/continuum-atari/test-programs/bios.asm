; A stand-in for the 7800's BIOS, of our own: MAME's 7800 won't start without a BIOS file, and
; the real one is Atari's, so it is never fetched. This does the one thing the real BIOS's last
; steps do for a 7800 cartridge: switch the BIOS out, turn MARIA on, lock INPTCTRL, and jump to
; the cartridge's reset vector. cAtari runs the same file in the tests, so both start alike.

        processor 6502
        include "hw.inc"

        org $F000
start:
        sei
        cld
        ldx #$FF
        txs
        ldx #0
copy:   lda tramp,x                 ; the switch has to run from RAM: once the BIOS is out,
        sta $2300,x                 ; the cartridge's ROM is what $F000 shows
        inx
        cpx #tramp_end-tramp
        bne copy
        lda #$07                    ; lock, MARIA on, BIOS out
        jmp $2300
tramp:
        sta INPTCTRL
        jmp ($FFFC)
tramp_end:

        org $FFFA
        .word start, start, start
