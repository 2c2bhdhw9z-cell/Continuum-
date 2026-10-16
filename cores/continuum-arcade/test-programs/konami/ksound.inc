; The sound board's test program (GPL-3.0-only, cArcade's own): Konami's sound Z80 with its two
; AY-3-8910s (Frogger's one, with FROGSND defined). It plays a script of tones, one or two
; channels at a time, with each filter switch setting, then all six at once and the noise, each
; segment SEGMENT turns of the timer's top bit long (about 0.2 s), timing itself on the timer
; through the first AY's port B. The interrupt handler reads each command from the latch through
; port A and keeps it and a count of them in RAM, so a trace of the CPU shows them arriving.
; 4 KB at most: The End's board has the least sound ROM.

    DEVICE NOSLOT64K
    IFDEF FROGSND
SRAM    equ 0x4000
FILT    equ 0x6000      ; a write here sets the filter switches from address bits 0-11
AY0A    equ 0x80        ; the AY's address latch is on A7, its data on A6
AY0D    equ 0x40
    ELSE
SRAM    equ 0x8000
FILT    equ 0x9000
AY0A    equ 0x40        ; the first AY: address on A6, data on A7
AY0D    equ 0x80
AY1A    equ 0x10        ; the second: address on A4, data on A5
AY1D    equ 0x20
    ENDIF
CMD     equ SRAM        ; the last command
CMDS    equ SRAM + 1    ; how many came
SEGMENT equ 140

    MACRO AW0 reg, value
    di
    ld a, reg
    out (AY0A), a
    ld a, value
    out (AY0D), a
    ld a, 15
    out (AY0A), a
    ei
    ENDM

    MACRO AW1 reg, value
    di
    ld a, reg
    out (AY1A), a
    ld a, value
    out (AY1D), a
    ei
    ENDM

    MACRO FLT bits
    ld (FILT + bits), a
    ENDM

    MACRO WAIT turns
    ld b, turns
    call wait
    ENDM

    org 0
    di
    ld sp, SRAM + 0x400
    im 1
    jp start

    org 0x38
irq:
    push af
    ld a, 14
    out (AY0A), a
    in a, (AY0D)            ; the command
    ld (CMD), a
    push hl
    ld hl, (CMDS)
    inc hl
    ld (CMDS), hl
    pop hl
    ld a, 15                ; the timer again, for the main loop
    out (AY0A), a
    pop af
    ei
    ret

start:
    AW0 7, 0x38             ; tones on, noise off, both ports inputs
    AW0 8, 0
    AW0 9, 0
    AW0 10, 0
    IFNDEF FROGSND
    AW1 7, 0x38
    AW1 8, 0
    AW1 9, 0
    AW1 10, 0
    ENDIF
    FLT 0
    ei
    ; silence
    WAIT SEGMENT
    ; AY1 A 060h
    AW0 0, 0x60
    AW0 1, 0x00
    AW0 8, 0x0F
    WAIT SEGMENT
    ; AY1 A 060h, 0.22uF
    FLT 0x040
    WAIT SEGMENT
    ; AY1 A 060h, 0.047uF
    FLT 0x080
    WAIT SEGMENT
    ; AY1 A 060h, both
    FLT 0x0C0
    WAIT SEGMENT
    ; AY1 B 0E0h
    FLT 0x000
    AW0 8, 0x00
    AW0 2, 0xE0
    AW0 3, 0x00
    AW0 9, 0x0F
    WAIT SEGMENT
    ; AY1 B 0E0h, 0.22uF
    FLT 0x100
    WAIT SEGMENT
    ; AY1 B 0E0h, 0.047uF
    FLT 0x200
    WAIT SEGMENT
    ; AY1 B 0E0h, both
    FLT 0x300
    WAIT SEGMENT
    ; AY1 C 040h vol 12
    FLT 0x000
    AW0 9, 0x00
    AW0 4, 0x40
    AW0 5, 0x00
    AW0 10, 0x0C
    WAIT SEGMENT
    ; AY1 C 040h vol 12, both
    FLT 0xC00
    WAIT SEGMENT
    IFNDEF FROGSND
    ; AY2 A 030h
    FLT 0x000
    AW0 10, 0x00
    AW1 0, 0x30
    AW1 1, 0x00
    AW1 8, 0x0F
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 A 030h, 0.22uF
    FLT 0x001
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 B 150h vol 10
    FLT 0x000
    AW1 8, 0x00
    AW1 2, 0x50
    AW1 3, 0x01
    AW1 9, 0x0A
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 B 150h vol 10, 0.047uF
    FLT 0x008
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 C 020h
    FLT 0x000
    AW1 9, 0x00
    AW1 4, 0x20
    AW1 5, 0x00
    AW1 10, 0x0F
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 C 020h, both
    FLT 0x030
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY1 A and AY2 B
    FLT 0x000
    AW1 10, 0x00
    AW0 0, 0x60
    AW0 1, 0x00
    AW0 8, 0x0F
    AW1 2, 0x50
    AW1 3, 0x01
    AW1 9, 0x0F
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; all six
    AW0 2, 0xE0
    AW0 3, 0x00
    AW0 9, 0x0F
    AW0 4, 0x40
    AW0 5, 0x00
    AW0 10, 0x0F
    AW1 0, 0x30
    AW1 1, 0x00
    AW1 8, 0x0F
    AW1 4, 0x20
    AW1 5, 0x00
    AW1 10, 0x0F
    WAIT SEGMENT
    ENDIF
    IFNDEF FROGSND
    ; AY2 noise on A
    AW0 8, 0x00
    AW0 9, 0x00
    AW0 10, 0x00
    AW1 8, 0x00
    AW1 9, 0x00
    AW1 10, 0x00
    AW1 6, 0x0F
    AW1 7, 0x37
    AW1 8, 0x0F
    WAIT SEGMENT
    ENDIF
    ; end
    AW0 8, 0x00
    AW0 9, 0x00
    AW0 10, 0x00
    IFNDEF FROGSND
    AW1 8, 0x00
    AW1 9, 0x00
    AW1 10, 0x00
    AW1 7, 0x38
    ENDIF
    WAIT SEGMENT
.forever:
    WAIT 1
    jr .forever

; Waits for B turns of the timer's top bit (each 2560 CPU cycles), with R15 selected.
wait:
    in a, (AY0D)
    and 0x80
    ld c, a
.w: in a, (AY0D)
    and 0x80
    cp c
    jr z, .w
    ld c, a
    djnz .w
    ret

    ASSERT $ <= 0x1000
