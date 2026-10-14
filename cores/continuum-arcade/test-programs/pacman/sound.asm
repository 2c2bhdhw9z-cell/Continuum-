; Pac-Man board test: the Namco sound chip (GPL-3.0-only, cArcade's own).
; Sound goes on at frame 2. Each frame, in the interrupt:
;   voice 0: its 20-bit frequency (all five nibbles, the low one only voice 0 has) climbs by
;            0A37h, its waveform steps through all eight every 64 frames, its volume through
;            0-15 (0 is silent);
;   voice 1: a frequency from a table of 16, a waveform, and a volume falling from 15;
;   voice 2: high pitches near the top of the range, silent for frames 150-159;
;   and junk goes to the nibbles the hardware keeps its phase counters in.
; Sound is switched off for frames 100-105, every voice set to volume 0 just before each switch:
; MAME applies the switch from its sound stream's last update rather than from the moment of
; the write, which only sounds the same while every voice is silent. After each interrupt the main loop waits a different
; time every frame and then sets voice 1's volume, so writes land all through the frame.
; Every value is written with junk in its top nibble, which the chip ignores.

    DEFINE OUTNAME "sound.bin"
    include "hw.inc"
    include "start.inc"

F0      equ USER        ; 3 bytes: voice 0's 20-bit frequency
F2      equ USER + 4    ; 2 bytes: voice 2's frequency, top 16 of 20 bits

init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x7FF
    ld (hl), 0
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ld hl, 0xF000
    ld (F2), hl
    jp run

; Writes A's low nibble to (HL) and its high nibble to (HL+1); HL moves on two.
wnib:
    ld (hl), a
    inc hl
    rrca
    rrca
    rrca
    rrca
    ld (hl), a
    inc hl
    ret

frame:
    push hl
    ; the sound enable: on at 2, off at 100, on again at 106
    ld a, h
    or a
    jr nz, .voices
    ld a, l
    cp 2
    jr z, .en
    cp 106
    jr z, .en
    cp 100
    jr nz, .voices
    xor a
    jr .set
.en: ld a, 0xF1
.set:
    ; every voice to volume 0 first (see the note at the top)
    ld b, a
    xor a
    ld (SOUND + S_VOL0), a
    ld (SOUND + S_VOL1), a
    ld (SOUND + S_VOL2), a
    ld a, b
    ld (SNDEN), a
.voices:
    pop hl
    push hl
    ; voice 0: frequency + 0A37h, all five nibbles
    ld hl, (F0)
    ld de, 0x0A37
    add hl, de
    ld (F0), hl
    ld a, (F0 + 2)
    adc a, 0
    and 0x0F
    ld (F0 + 2), a
    ld hl, SOUND + S_FREQ0
    ld a, (F0)
    call wnib
    ld a, (F0 + 1)
    call wnib
    ld a, (F0 + 2)
    or 0x50
    ld (hl), a
    pop hl
    push hl
    ; voice 0 waveform (frame / 8) and volume (frame / 2)
    ld a, l
    rrca
    rrca
    rrca
    or 0xE8
    ld (SOUND + S_WAVE0), a
    ld a, l
    rrca
    or 0x30
    ld (SOUND + S_VOL0), a
    ; voice 1: frequency from the table, entry (frame / 4) & 15
    ld a, l
    rrca
    and 0x1E
    ld e, a
    ld d, 0
    push hl
    ld hl, v1_freqs
    add hl, de
    ld e, (hl)
    inc hl
    ld d, (hl)
    ld hl, SOUND + S_FREQ1
    ld a, e
    call wnib
    ld a, d
    call wnib
    pop hl
    ; voice 1 waveform (frame / 16 + 3) and volume (15 - frame / 8)
    ld a, l
    rrca
    rrca
    rrca
    rrca
    add a, 3
    or 0x90
    ld (SOUND + S_WAVE1), a
    ld a, l
    rrca
    rrca
    rrca
    cpl
    ld (SOUND + S_VOL1), a
    ; voice 2: high pitches, frequency falls by 0123h a frame
    push hl
    ld hl, (F2)
    ld de, 0x0123
    or a
    sbc hl, de
    ld (F2), hl
    ex de, hl
    ld hl, SOUND + S_FREQ2
    ld a, e
    call wnib
    ld a, d
    call wnib
    pop hl
    ld a, l
    and 1
    add a, 6
    ld (SOUND + S_WAVE2), a
    ; silent for frames 150-159
    ld a, h
    or a
    ld a, 9
    jr nz, .v2
    ld a, l
    sub 150
    cp 10
    ld a, 9
    jr nc, .v2
    xor a
.v2: ld (SOUND + S_VOL2), a
    ; junk into the phase-counter nibbles, which the chip keeps for itself
    pop hl
    ld a, l
    ld (SOUND + 0x00), a
    ld (SOUND + 0x02), a
    ld (SOUND + 0x04), a
    ld (SOUND + 0x07), a
    ld (SOUND + 0x0B), a
    ld (SOUND + 0x0E), a
    ret

; After the interrupt: wait (frame * 97) & 3FFh loops, then set voice 1's volume.
idle:
    ld hl, (FRAMES)
    ld d, h
    ld e, l
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl
    add hl, hl              ; * 32
    push hl
    add hl, hl              ; * 64
    pop bc
    add hl, bc              ; * 96
    add hl, de              ; * 97
    ld a, h
    and 3
    ld b, a
    ld c, l
    inc bc
.w: dec bc
    ld a, b
    or c
    jr nz, .w
    ld a, e
    and 0x0F
    or 0x40
    ld (SOUND + S_VOL1), a
    ret

v1_freqs:
    dw 0x0123, 0x0456, 0x0789, 0x0ABC, 0x0DEF, 0x1234, 0x2345, 0x3456
    dw 0x00F0, 0x0F00, 0x4000, 0x7FFF, 0x8001, 0xC003, 0xFFFF, 0x0001

    include "end.inc"
