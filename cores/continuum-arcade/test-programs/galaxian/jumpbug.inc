; Jump Bug's memory map (Galaxian's video moved, 2 KB of RAM, five graphics bank latches, an
; AY-3-8910, no watchdog), for cArcade's own test programs (GPL-3.0-only). Included before hw.inc.
    DEFINE MOONCRESTA
RAM     equ 0x4000      ; 2 KB
STACK   equ 0x4800
VRAM    equ 0x4800      ; mirrored at 4C00h
OBJ     equ 0x5000      ; mirrored to 57FFh
AYDATA  equ 0x5800
AYADDR  equ 0x5900
IN0     equ 0x6000
IN1     equ 0x6800
IN2     equ 0x7000
GFXBANK equ 0x6002      ; 6002h-6006h
NMIEN   equ 0x7001
COUNTER equ 0x7002
STARS   equ 0x7004
FLIPX   equ 0x7006
FLIPY   equ 0x7007
WDOG    equ 0x7800      ; nothing here (no watchdog): reads FFh
PITCH   equ 0x7800
UNMAP   equ 0x7800
LFO     equ 0x7808      ; nothing (no Galaxian sound circuit)
SND     equ 0x7808
