; The Galaxian board's memory map, for cArcade's own test programs (GPL-3.0-only, like the core).
; mooncrst.inc has Moon Cresta's; the programs use these names on either.
    IFNDEF MOONCRESTA
RAM     equ 0x4000      ; 1 KB, mirrored at 4400h
STACK   equ 0x4400
VRAM    equ 0x5000      ; 32 x 32 tile codes, mirrored at 5400h
OBJ     equ 0x5800      ; object RAM, mirrored to 5FFFh
IN0     equ 0x6000      ; read: coins, P1 left, right, fire, cabinet, test, service
IN1     equ 0x6800      ; read: starts, P2 left, right, fire, coinage switches
IN2     equ 0x7000      ; read: DIP switches
LAMP1   equ 0x6000      ; write bit 0
LAMP2   equ 0x6001
LOCKOUT equ 0x6002
COUNTER equ 0x6003
LFO     equ 0x6004      ; 6004h-6007h: the background sound's sweep
SND     equ 0x6800      ; 6800h-6807h: FS1, FS2, FS3, HIT, -, FIRE, VOL1, VOL2
NMIEN   equ 0x7001      ; write bit 0: NMI on VBLANK
STARS   equ 0x7004
FLIPX   equ 0x7006
FLIPY   equ 0x7007
WDOG    equ 0x7800      ; read: kicks the watchdog
PITCH   equ 0x7800      ; write: the tone's pitch
UNMAP   equ 0x4800      ; nothing here
    ENDIF

; Object RAM
SCROLL  equ OBJ         ; column n: scroll at 2n, colour at 2n+1
SPR     equ OBJ + 0x40  ; sprite n: y, code | xflip << 6 | yflip << 7, colour, x
SHOT    equ OBJ + 0x60  ; shot n: -, y, -, x

; Work RAM the shared frame loop uses
FRAMES  equ RAM         ; 16-bit count of NMIs
BOOTS   equ RAM + 2     ; counts warm boots (the watchdog's resets keep RAM)
MAGIC   equ RAM + 3     ; 2 bytes: A5h 5Ah once RAM has been set up
NOKICK  equ RAM + 5     ; non-zero: the NMI handler stops kicking the watchdog
NMIOFF  equ RAM + 6     ; non-zero: the NMI handler leaves the NMI switched off
NMIKEEP equ RAM + 7     ; non-zero: the NMI handler leaves the line high
USER    equ RAM + 0x10  ; free for each program
