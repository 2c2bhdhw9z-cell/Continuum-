; The Pac-Man board's memory map, for cArcade's own test programs (GPL-3.0-only, like the core).
VRAM    equ 0x4000      ; tile codes, 1 KB
CRAM    equ 0x4400      ; tile colours, 1 KB
RAM     equ 0x4C00      ; work RAM to 4FEFh
SPRATTR equ 0x4FF0      ; sprites 0-7: code<<2 | yflip<<1 | xflip, then colour
STACK   equ 0x4FF0      ; the stack grows down from the sprite attributes

IN0     equ 0x5000      ; read: P1 stick, rack test, coins, service
IRQEN   equ 0x5000      ; write bit 0: VBLANK interrupt enable
SNDEN   equ 0x5001      ; write bit 0: sound enable
FLIP    equ 0x5003      ; write bit 0: flip screen
LAMP1   equ 0x5004
LAMP2   equ 0x5005
LOCKOUT equ 0x5006
COUNTER equ 0x5007
IN1     equ 0x5040      ; read: P2 stick, test switch, starts, cabinet
SOUND   equ 0x5040      ; write: 32 sound register nibbles
SPRXY   equ 0x5060      ; write: sprites 0-7 x, y
DSW1    equ 0x5080      ; read: DIP switches
DSW2    equ 0x50C0      ; read: second switch bank (nothing on Pac-Man)
WDOG    equ 0x50C0      ; write: kick the watchdog

; Sound registers (offsets from SOUND)
S_WAVE0 equ 0x05
S_WAVE1 equ 0x0A
S_WAVE2 equ 0x0F
S_FREQ0 equ 0x10        ; five nibbles, low first
S_VOL0  equ 0x15
S_FREQ1 equ 0x16        ; four nibbles
S_VOL1  equ 0x1A
S_FREQ2 equ 0x1B
S_VOL2  equ 0x1F

; Work RAM the shared frame loop uses
FRAMES  equ RAM         ; 16-bit count of VBLANK interrupts
BOOTS   equ RAM + 2     ; counts warm boots (the watchdog's resets keep RAM)
MAGIC   equ RAM + 3     ; 2 bytes: A5h 5Ah once RAM has been set up
NOKICK  equ RAM + 5     ; non-zero: the frame loop stops kicking the watchdog
USER    equ RAM + 0x10  ; free for each program
