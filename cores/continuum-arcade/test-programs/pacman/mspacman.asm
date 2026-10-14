; Pac-Man board test: Ms. Pac-Man's daughterboard (GPL-3.0-only, cArcade's own).
; The board boots switched to the daughterboard's program. Every frame this program, from its
; VBLANK interrupt (whose vector is fetched from 3FFEh, inside the switch-on trap, so every
; interrupt switches it back on):
;   - asks which program shows (a CALL to 8800h: the daughterboard's u6 answers A5h, the plain
;     board's mirror of 0800h answers 5Ah);
;   - goes through every trap: switch-off reads at 0038h, 8000h, 97F0h, 2120h and 3FF0h, a
;     switch-off write at 03B0h, a switch-off instruction fetch at 1600h, and switch-on by a
;     read, a write and an instruction fetch in 3FF8h-3FFFh, asking which program shows after
;     each;
;   - reads bytes of five of the 40 patched places, and of every part of the address map, in
;     both modes;
;   - adds up the program from 0000h the way Pac-Man's self-test does, which hits the trap at
;     0038h first, so it adds the plain program;
; and shows every answer as a tile. The assembled image holds the two views: 0000h-3FFFh the
; Pac-Man ROMs (pacman.6e-6j), and u5/u6/u7 as the Z80 sees them once decoded; the test
; scrambles those the way the daughterboard's wiring unscrambles them.

    DEVICE NOSLOT64K
    include "hw.inc"

OUT     equ USER        ; the output pointer

; ---- the Pac-Man ROMs, 0000h-3FFFh ------------------------------------------------------
    org 0
    di
    ld sp, STACK
    jp init

    org 0x0040
init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3FF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld de, VRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x44
    ldir
    ld hl, CRAM
    ld de, CRAM + 1
    ld bc, 0x3FF
    ld (hl), 0x09
    ldir
    ld hl, SPRXY
    ld b, 16
.s: ld (hl), 0
    inc hl
    djnz .s
    ld a, 0x3F
    ld i, a
    im 2
    ld a, 0xFE
    out (0), a
    ld a, 1
    ld (IRQEN), a
    ei
.loop:
    halt
    jr .loop

vblank:
    push af
    push bc
    push de
    push hl
    xor a
    ld (IRQEN), a
    ld (WDOG), a
    ld hl, (FRAMES)
    inc hl
    ld (FRAMES), hl
    ld a, l
    ld (VRAM + 0x3C4), a
    call tests
    ld a, 1
    ld (IRQEN), a
    pop hl
    pop de
    pop bc
    pop af
    ei
    reti

; Shows A at the output pointer and moves it on.
put:
    push hl
    ld hl, (OUT)
    ld (hl), a
    inc hl
    ld (OUT), hl
    pop hl
    ret

; Which program shows: A5h the daughterboard's, 5Ah plain Pac-Man.
which:
    call 0x8800
    jr put

    org 0x0418
tests:
    ld hl, VRAM + 0x040
    ld (OUT), hl
    call which              ; A5: on at boot and after the interrupt
    ld a, (0x0038)          ; off, by a read
    call put
    call which
    ld a, (0x3FF9)          ; on, by a read
    call put
    call which
    ld (0x03B4), a          ; off, by a write
    call which
    ld (0x3FFC), a          ; on, by a write
    call which
    call 0x1600             ; off, by an instruction fetch (a RET there)
    call which
    call 0x3FF8             ; on, by an instruction fetch (a RET in u7)
    call which
    ld a, (0x8003)          ; off at 8000h
    call put
    call which
    ld a, (0x3FF8)
    ld a, (0x97F5)          ; off at 97F0h
    call put
    call which
    ld a, (0x3FF8)
    call 0x2120             ; off at 2120h, by a fetch
    call which
    ld a, (0x3FF8)
    ld a, (0x3FF3)          ; off at 3FF0h, just under the switch-on trap
    call put
    call which
    ld a, (0x3FF8)
    ; the patched places, and every part of the map, on then off
    ld hl, VRAM + 0x060
    ld (OUT), hl
    call places
    ld a, (0x0038)
    call places
    ld a, (0x3FF8)
    ; a checksum from 0000h up, as Pac-Man's self-test walks it (512 bytes, to fit in a frame)
    ld hl, 0
    ld de, 0
    ld bc, 0x0200
.sum:
    ld a, (hl)
    add a, e
    ld e, a
    ld a, d
    adc a, 0
    ld d, a
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .sum
    ld a, e
    call put
    ld a, d
    call put
    call which              ; still off after the checksum
    ld a, (0x3FF8)
    call which
    ret

places:
    ld hl, place_list
    ld b, (place_end - place_list) / 2
.p: ld e, (hl)
    inc hl
    ld d, (hl)
    inc hl
    ld a, (de)
    call put
    djnz .p
    ret

place_list:
    dw 0x0410, 0x0417, 0x08E0, 0x2B27, 0x2D60   ; patched places
    dw 0x0123, 0x1123, 0x2123, 0x3123           ; Pac-Man ROMs / u7
    dw 0x8123, 0x8923, 0x9123, 0x9923           ; u5, u6 (high half), u6 (low half), 6f
    dw 0xA123, 0xB123                           ; 6h, 6j mirrors
place_end:

    org 0x0800
    ld a, 0x5A              ; plain Pac-Man's answer at 0800h (8800h when switched off)
    ret

; Plain bytes at the five patched places the test reads, and at 1600h and 2120h a RET.
    org 0x0410
    db 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17
    org 0x08E0
    db 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F
    org 0x2B20
    db 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F
    org 0x2D60
    db 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37
    org 0x1600
    ret
    org 0x2120
    ret
    org 0x0123
    db 0x61
    org 0x1123
    db 0x62
    org 0x2123
    db 0x63
    org 0x3123
    db 0x64
    org 0x3FF8
    ret
    org 0x3FFE
    dw vblank

; ---- u7 as the Z80 sees it, 3000h-3FFFh (stored at C000h here) --------------------------
    org 0xC000
    DISP 0x3000
    DS 0x123, 0x71
    db 0x74
    DS 0x3FF8 - 0x3124, 0x72
    ret                     ; 3FF8h: the switch-on fetch lands here
    DS 5, 0x73
    dw vblank               ; 3FFEh: the interrupt vector's table entry
    ENT

; ---- u5 and u6 as the Z80 sees them, 8000h-97FFh ----------------------------------------
    org 0x8000
n = 0
    DUP 0x1F0               ; the patches' source bytes
    db (n * 5 + 0x33) & 0xFF
n = n + 1
    EDUP
    org 0x8123
    db 0x81
    org 0x8800
    ld a, 0xA5              ; the daughterboard's answer at 8800h
    ret
    org 0x8923
    db 0x89
    org 0x9123
    db 0x91

    SAVEBIN "mspacman.bin", 0, 0x4000
    SAVEBIN "mspacman-u7.bin", 0xC000, 0x1000
    SAVEBIN "mspacman-u56.bin", 0x8000, 0x1800
