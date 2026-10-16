; Donkey Kong Jr. board test: the sound (GPL-3.0-only, cArcade's own), the main CPU's side; the
; 8035 runs jrsnd.a48. The script sends each of the 8035's commands in turn (some with the
; interrupt), sets latch 6H's bits 3-6 and latch 4H's bit 1 one by one under command 4 (they reach
; the 8035's port 2 bits 5, 4 and 6 and T0, T1), then plays the board's analogue sounds: footsteps
; (latch 6H bit 0 falling), footsteps with bit 7 set, the jump (bit 1), the climb (bit 2), the fall
; (latch 5H bit 1) and all of them over a tone. The 8257 copies sprites every frame meanwhile. The
; frames are NMIs counted; Jr.'s command latch isn't inverted.

    DEFINE JUNIOR
    DEFINE SOUND_DMA
    DEFINE OUTNAME "jrsound.bin"
    include "hw.inc"
    include "start.inc"
    include "sound.inc"

script:
    STEP 6, S_SEG, 1
    STEP 6, S_CMD, 1
    STEP 40, S_SEG, 2
    STEP 40, S_CMD, 2
    STEP 40, S_IRQ, 1
    STEP 42, S_IRQ, 0
    STEP 74, S_SEG, 3
    STEP 74, S_CMD, 3
    STEP 108, S_SEG, 4
    STEP 108, S_CMD, 4
    STEP 120, S_SEG, 5
    STEP 120, S_6H, 0x80 + 5
    STEP 132, S_SEG, 6
    STEP 132, S_6H, 0x80 + 4
    STEP 144, S_SEG, 7
    STEP 144, S_6H, 0x80 + 6
    STEP 156, S_SEG, 8
    STEP 156, S_6H, 0x80 + 3
    STEP 168, S_SEG, 9
    STEP 168, S_4H, 0x80 + 1
    STEP 180, S_SEG, 10
    STEP 180, S_6H, 5
    STEP 180, S_6H, 4
    STEP 180, S_6H, 6
    STEP 180, S_6H, 3
    STEP 180, S_4H, 1
    STEP 192, S_SEG, 11
    STEP 192, S_CMD, 5
    STEP 192, S_IRQ, 1
    STEP 193, S_IRQ, 0
    STEP 228, S_SEG, 12
    STEP 228, S_CMD, 6
    STEP 276, S_SEG, 13
    STEP 276, S_CMD, 7
    STEP 312, S_SEG, 14
    STEP 312, S_CMD, 8
    STEP 348, S_SEG, 15
    STEP 348, S_CMD, 9
    STEP 366, S_CMD, 0x19
    STEP 384, S_SEG, 16
    STEP 384, S_CMD, 0
    STEP 400, S_SEG, 17
    STEP 400, S_6H, 0x80 + 0
    STEP 402, S_6H, 0
    STEP 412, S_6H, 0x80 + 0
    STEP 414, S_6H, 0
    STEP 424, S_6H, 0x80 + 0
    STEP 426, S_6H, 0
    STEP 436, S_6H, 0x80 + 0
    STEP 438, S_6H, 0
    STEP 450, S_SEG, 18
    STEP 450, S_6H, 0x80 + 7
    STEP 452, S_6H, 0x80 + 0
    STEP 454, S_6H, 0
    STEP 464, S_6H, 0x80 + 0
    STEP 466, S_6H, 0
    STEP 476, S_6H, 0x80 + 0
    STEP 478, S_6H, 0
    STEP 490, S_6H, 7
    STEP 500, S_SEG, 19
    STEP 500, S_6H, 0x80 + 1
    STEP 502, S_6H, 1
    STEP 530, S_6H, 0x80 + 1
    STEP 532, S_6H, 1
    STEP 560, S_SEG, 20
    STEP 560, S_6H, 0x80 + 2
    STEP 562, S_6H, 2
    STEP 590, S_6H, 0x80 + 2
    STEP 592, S_6H, 2
    STEP 620, S_SEG, 21
    STEP 620, S_5H, 0x80 + 1
    STEP 660, S_5H, 1
    STEP 670, S_SEG, 22
    STEP 670, S_CMD, 1
    STEP 670, S_5H, 0x80 + 1
    STEP 672, S_6H, 0x80 + 0
    STEP 674, S_6H, 0
    STEP 676, S_6H, 0x80 + 1
    STEP 678, S_6H, 1
    STEP 686, S_6H, 0x80 + 2
    STEP 688, S_6H, 2
    STEP 696, S_6H, 0x80 + 0
    STEP 698, S_6H, 0
    STEP 720, S_SEG, 23
    STEP 720, S_CMD, 0
    STEP 720, S_5H, 1
    DB S_END

    include "end.inc"
