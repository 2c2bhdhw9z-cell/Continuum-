; Pac-Man board test: the Pengo bootlegs rebuilt on a Pac-Man board (GPL-3.0-only, cArcade's own).
; Their board moves the RAM and ports up: video RAM at 8000h, colours at 8400h, RAM at 8800h-8FFFh
; and F000h-FFFFh, the latch, sound, sprites, ports and watchdog at 9000h-90FFh, each at one
; address with no mirrors, and the program in 0000h-0FFFh and 4000h-7FFFh. This program draws
; tiles and moving sprites, plays a tone, and shows what each port and a few unmapped addresses
; read, and that both RAMs hold what was written.

    DEVICE NOSLOT64K

VRAM    equ 0x8000
CRAM    equ 0x8400
RAM2    equ 0x8800
RAM     equ 0x8C00
SPRATTR equ 0x8FF0
IRQEN   equ 0x9000
SNDEN   equ 0x9001
FLIP    equ 0x9003
SOUND   equ 0x9040
SPRXY   equ 0x9060
WDOG    equ 0x90C0
HIGHRAM equ 0xF000
FRAMES  equ RAM

    org 0
    di
    ld sp, SPRATTR
    jp init

    org 0x4000
init:
    ld hl, RAM
    ld de, RAM + 1
    ld bc, 0x3EF
    ld (hl), 0
    ldir
    ld hl, VRAM
    ld b, 0
    ld c, 4
.v: ld a, l
    add a, h
    ld (hl), a
    inc hl
    djnz .v
    dec c
    jr nz, .v
    ld hl, CRAM
    ld bc, 0x400
.c: ld a, l
    rrca
    rrca
    ld (hl), a
    inc hl
    dec bc
    ld a, b
    or c
    jr nz, .c
    ld a, 0x5A
    ld (RAM2 + 0x123), a
    ld a, 0xA7
    ld (HIGHRAM + 0xABC), a
    ; a tone on voice 0
    ld hl, SOUND
    ld b, 0x20
.z: ld (hl), 0
    inc hl
    djnz .z
    ld a, 2
    ld (SOUND + 0x05), a
    ld a, 0x0C
    ld (SOUND + 0x13), a
    ld a, 0x0F
    ld (SOUND + 0x15), a
    ld a, 1
    ld (SNDEN), a
    ld a, 0x4F
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
    ; sprites
    ld b, 8
    ld de, SPRATTR
    ld hl, SPRXY
.s: ld a, b
    add a, a
    add a, a
    add a, a
    add a, a
    add a, a
    ld c, a
    ld a, (FRAMES)
    add a, c
    ld (hl), a
    inc hl
    ld a, c
    add a, 30
    ld (hl), a
    inc hl
    ld a, (FRAMES)
    add a, b
    add a, a
    add a, a
    ld (de), a
    inc de
    ld a, b
    ld (de), a
    inc de
    djnz .s
    ; the ports and what's around them, into the playfield's first column
    ld hl, VRAM + 0x040
    ld a, (0x9000)
    ld (hl), a
    inc hl
    ld a, (0x9040)
    ld (hl), a
    inc hl
    ld a, (0x9080)
    ld (hl), a
    inc hl
    ld a, (0x90C0)
    ld (hl), a
    inc hl
    ld a, (0x9001)
    ld (hl), a
    inc hl
    ld a, (0x903F)
    ld (hl), a
    inc hl
    ld a, (0x5000)
    ld (hl), a
    inc hl
    ld a, (0x1000)
    ld (hl), a
    inc hl
    ld a, (0x3FFF)
    ld (hl), a
    inc hl
    ld a, (RAM2 + 0x123)
    ld (hl), a
    inc hl
    ld a, (HIGHRAM + 0xABC)
    ld (hl), a
    inc hl
    ld a, (0x0123)
    ld (hl), a
    inc hl
    ld a, (0x4FF0)
    ld (hl), a
    inc hl
    ld a, (0xC000)
    ld (hl), a
    inc hl
    ld a, (FRAMES)
    ld (hl), a
    ; writes that land nowhere: 9008h (no latch mirror), 5000h, C040h
    ld a, 1
    ld (0x900B), a
    ld (0x5000), a
    ld (0xC040), a
    ; the flip from frame 64 to 95
    ld a, (FRAMES)
    and 0xE0
    cp 0x40
    ld a, 0
    jr nz, .f
    inc a
.f: ld (FLIP), a
    ld a, 1
    ld (IRQEN), a
    pop hl
    pop de
    pop bc
    pop af
    ei
    reti

    org 0x4FFE
    dw vblank

    SAVEBIN "pengo.bin", 0, 0x8000
