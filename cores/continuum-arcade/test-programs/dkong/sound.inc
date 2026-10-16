; The main CPU's side of the Donkey Kong-board sound tests (GPL-3.0-only, cArcade's own): it plays
; a script, a list of (frame, what, value) that the program including this file supplies as
; `script`, one step at the start of the frame named:
;   S_CMD   the sound command latch (7C00h)
;   S_IRQ   the 8035's /INT (7D80h)
;   S_6H    one bit of latch 6H: value = the bit's number, plus 80h to set it (7D00h-7D07h)
;   S_4H    on Donkey Kong Jr.'s board one bit of latch 4H (7C80h-7C87h), likewise
;   S_5H    on Donkey Kong Jr.'s board one bit of latch 5H (7D80h-7D87h; 7D81h is the fall sound)
;   S_SEG   the segment number the tests tell the sound's stretches apart by (SEG)
; Each frame it shows IN2 (bit 6 is the 8035's status bit on Donkey Kong's board), the segment
; and the frame count as tiles. With SOUND_DMA defined it also has the 8257 copy a few moving
; sprites every frame, as the games do, so the DMA's hold on the bus meets the sound's timing.

SEG     equ USER            ; the script's segment
SPOS    equ USER + 2        ; 2 bytes: where the script is

S_CMD   equ 0
S_IRQ   equ 1
S_6H    equ 2
S_4H    equ 3
S_5H    equ 4
S_SEG   equ 5
S_END   equ 0xFF

init:
    ld hl, VRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, SPRRAM
    ld bc, 0x400
    xor a
    call fill
    ld hl, script
    ld (SPOS), hl
    IFDEF SOUND_DMA
    ld hl, SPRLIST
    ld bc, 0x180
    xor a
    call fill
    ld ix, SPRLIST
    ld b, 8
    ld c, 40
.s: ld (ix + 0), c
    ld a, c
    add a, 0x23
    ld (ix + 1), a
    ld (ix + 2), b
    ld (ix + 3), c
    ld a, c
    add a, 21
    ld c, a
    ld de, 4
    add ix, de
    djnz .s
    ld a, 1
    ld (DMAON), a
    ENDIF
    jp run

frame:
    ex de, hl               ; DE = the frame count
    ld hl, (SPOS)
.next:
    ld a, (hl)              ; a step is: what, its frame (2 bytes), the value
    cp S_END
    jr z, .done
    inc hl
    ld c, (hl)              ; BC = the step's frame
    inc hl
    ld b, (hl)
    dec hl
    dec hl
    ld a, c
    cp e
    jr nz, .done
    ld a, b
    cp d
    jr nz, .done
    ld a, (hl)              ; what
    inc hl
    inc hl
    inc hl
    ld c, (hl)              ; value
    inc hl
    push hl
    call step
    pop hl
    jr .next
.done:
    ld (SPOS), hl
    IFDEF SOUND_DMA
    ; The sprites move.
    ld ix, SPRLIST
    ld b, 8
.m: inc (ix + 0)
    dec (ix + 3)
    ld de, 4
    add ix, de
    djnz .m
    ENDIF
    ld hl, VRAM + 8 * 32 + 12
    ld a, (IN2)
    call bits
    ld a, (SEG)
    call hex
    ld a, (FRAMES + 1)
    call hex
    ld a, (FRAMES)
    call hex
    ret

; Does step A with value C (DE, the frame count, is kept).
step:
    or a
    jr nz, .irq
    ld a, c
    ld (SNDCMD), a
    ret
.irq:
    dec a
    jr nz, .l6h
    ld a, c
    ld (SNDIRQ), a
    ret
.l6h:
    ld hl, LATCH6H
    dec a
    jr z, .bit
    ld hl, LATCH4H
    dec a
    jr z, .bit
    ld hl, LATCH5H
    dec a
    jr z, .bit
    ld a, c
    ld (SEG), a
    ret
.bit:
    push de
    ld a, c
    and 7
    ld e, a
    ld d, 0
    add hl, de
    pop de
    ld a, c
    rlca
    and 1
    ld (hl), a
    ret

idle:
    ret

; A script step: what, the frame, the value.
    MACRO STEP frame, what, value
    DB what
    DW frame
    DB value
    ENDM
