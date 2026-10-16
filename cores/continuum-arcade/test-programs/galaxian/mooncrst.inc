; Moon Cresta's memory map (the Galaxian board with ROM, RAM and video moved), for cArcade's own
; test programs (GPL-3.0-only). Included before hw.inc.
    DEFINE MOONCRESTA
RAM     equ 0x8000      ; 1 KB, mirrored at 8400h
STACK   equ 0x8400
VRAM    equ 0x9000      ; mirrored at 9400h
OBJ     equ 0x9800      ; mirrored to 9FFFh
IN0     equ 0xA000
IN1     equ 0xA800
IN2     equ 0xB000
GFXBANK equ 0xA000      ; A000h-A002h: the graphics bank latches (MAME keeps the whole byte)
COUNTER equ 0xA003
LFO     equ 0xA004
SND     equ 0xA800
NMIEN   equ 0xB000
STARS   equ 0xB004
FLIPX   equ 0xB006
FLIPY   equ 0xB007
WDOG    equ 0xB800
PITCH   equ 0xB800
UNMAP   equ 0x4000      ; nothing here
