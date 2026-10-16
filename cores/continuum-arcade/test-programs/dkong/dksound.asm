; Donkey Kong board test: the sound (GPL-3.0-only, cArcade's own), the main CPU's side; the 8035
; runs dksnd.a48. The script sends each of the 8035's commands in turn, some by polling and some
; with the interrupt (including one while the 8035 has the tune ROM paged in), sets latch 6H's
; bits 3-5 one by one under command 8 (they reach the 8035's T0, T1 and port 2 bit 5), then plays
; the board's analogue sounds from latch 6H: footsteps (bit 0), the jump (bit 1), the stomp
; (bit 2), and all of them over a tone. The frames are NMIs counted. The command byte is written
; with its low four bits inverted, as the 8035 reads them back.

    DEFINE OUTNAME "dksound.bin"
    include "hw.inc"
    include "start.inc"
    include "sound.inc"

C   equ 0x0F                ; the board inverts the command's low four bits

script:
    STEP 6, S_SEG, 1
    STEP 6, S_CMD, 1 ^ C
    STEP 40, S_SEG, 2
    STEP 40, S_CMD, 2 ^ C
    STEP 40, S_IRQ, 1
    STEP 42, S_IRQ, 0
    STEP 74, S_SEG, 3
    STEP 74, S_CMD, 3 ^ C
    STEP 108, S_SEG, 4
    STEP 108, S_CMD, 4 ^ C
    STEP 108, S_IRQ, 1
    STEP 109, S_IRQ, 0
    STEP 142, S_SEG, 5
    STEP 142, S_CMD, 5 ^ C
    STEP 176, S_SEG, 6
    STEP 176, S_CMD, 6 ^ C
    STEP 214, S_SEG, 7
    STEP 214, S_CMD, 7 ^ C
    STEP 214, S_IRQ, 1
    STEP 216, S_IRQ, 0
    STEP 262, S_SEG, 8
    STEP 262, S_CMD, 8 ^ C
    STEP 274, S_SEG, 9
    STEP 274, S_6H, 0x80 + 5
    STEP 286, S_SEG, 10
    STEP 286, S_6H, 0x80 + 4
    STEP 298, S_SEG, 11
    STEP 298, S_6H, 0x80 + 3
    STEP 310, S_SEG, 12
    STEP 310, S_6H, 5
    STEP 322, S_SEG, 13
    STEP 322, S_6H, 4
    STEP 334, S_SEG, 14
    STEP 334, S_6H, 3
    STEP 346, S_SEG, 15
    STEP 346, S_CMD, 9 ^ C
    STEP 382, S_SEG, 16
    STEP 382, S_CMD, 10 ^ C
    STEP 418, S_SEG, 17
    STEP 418, S_CMD, 0 ^ C
    STEP 442, S_SEG, 18
    STEP 442, S_6H, 0x80 + 0
    STEP 445, S_6H, 0
    STEP 454, S_6H, 0x80 + 0
    STEP 457, S_6H, 0
    STEP 466, S_6H, 0x80 + 0
    STEP 469, S_6H, 0
    STEP 478, S_6H, 0x80 + 0
    STEP 481, S_6H, 0
    STEP 492, S_SEG, 19
    STEP 512, S_SEG, 20
    STEP 512, S_6H, 0x80 + 1
    STEP 552, S_SEG, 21
    STEP 552, S_6H, 1
    STEP 582, S_SEG, 22
    STEP 582, S_6H, 0x80 + 2
    STEP 586, S_6H, 2
    STEP 602, S_6H, 0x80 + 2
    STEP 606, S_6H, 2
    STEP 622, S_SEG, 23
    STEP 622, S_CMD, 1 ^ C
    STEP 622, S_6H, 0x80 + 1
    STEP 628, S_6H, 0x80 + 0
    STEP 631, S_6H, 0
    STEP 640, S_6H, 0x80 + 0
    STEP 643, S_6H, 0
    STEP 646, S_6H, 0x80 + 2
    STEP 650, S_6H, 2
    STEP 672, S_SEG, 24
    STEP 672, S_CMD, 0 ^ C
    STEP 672, S_6H, 1
    DB S_END

    include "end.inc"
