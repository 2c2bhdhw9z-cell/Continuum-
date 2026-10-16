; The Donkey Kong board as cArcade's test programs use it (GPL-3.0-only, cArcade's own).

RAM     equ 0x6000
STACK   equ 0x6C00
SPRRAM  equ 0x7000          ; 7000h-73FFh: two banks of 128 sprites
VRAM    equ 0x7400          ; 7400h-77FFh: the 32 x 32 tile map
DMA     equ 0x7800          ; the 8257's registers
IN0     equ 0x7C00          ; reads: player 1; writes: the sound command latch
SNDCMD  equ 0x7C00
IN1     equ 0x7C80          ; reads: player 2; writes (Jr.): the tile bank
GFXBANK equ 0x7C80
LATCH4H equ 0x7C80          ; Jr.: 7C81h-7C87h, bit n of latch 4H
IN2     equ 0x7D00          ; reads: starts, coin (and the watchdog); writes: bit n of latch 6H
LATCH6H equ 0x7D00
DSW0    equ 0x7D80          ; reads: the DIP switches; writes: the 8035's /INT
SNDIRQ  equ 0x7D80
LATCH5H equ 0x7D80          ; Jr.: 7D81h, bit 1 of latch 5H
FLIP    equ 0x7D82
SPRBANK equ 0x7D83
NMIMASK equ 0x7D84
DRQ     equ 0x7D85
PALBANK equ 0x7D86          ; and 7D87h

; The frame loop's variables.
FRAMES  equ RAM + 0x00      ; 2 bytes: NMIs taken
MAGIC   equ RAM + 0x02      ; 2 bytes: 5AA5h once booted (a watchdog reset keeps RAM)
BOOTS   equ RAM + 0x04      ; warm boots
NOKICK  equ RAM + 0x05      ; nonzero: the NMI doesn't read IN2
NMIKEEP equ RAM + 0x06      ; nonzero: the NMI leaves the line high (no more NMIs)
NMIOFF  equ RAM + 0x07      ; nonzero: the NMI leaves the latch off
DMAON   equ RAM + 0x08      ; nonzero: the NMI copies the sprite list each frame
IDLES   equ RAM + 0x0A      ; 2 bytes: idle loops run
USER    equ RAM + 0x10
SPRLIST equ RAM + 0x900     ; the sprite list the DMA copies (96 sprites), as the game has it
