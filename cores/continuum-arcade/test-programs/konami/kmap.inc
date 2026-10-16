; Konami's Scramble-family boards: each memory map, for cArcade's own test programs (GPL-3.0-only,
; like the core). The program defines one of THEEND (Scramble's and The End's map), SCOBRA,
; FROGGER or TURTLES (Amidar's) before including this.
; PPI0 reads the controls (A IN0, B IN1, C IN2); PPI1 sends the sound board its command (A) and
; control (B: bit 3 falling interrupts it, bit 4 silences it) and reads IN3 or the protection on C.

    IFDEF THEEND
RAM     equ 0x4000      ; 2 KB
STACK   equ 0x4800
VRAM    equ 0x4800      ; mirrored at 4C00h
OBJ     equ 0x5000      ; mirrored to 57FFh
NMIEN   equ 0x6801
COIN    equ 0x6802
BGEN    equ 0x6803      ; Scramble's blue background
STARS   equ 0x6804
FLIPX   equ 0x6806
FLIPY   equ 0x6807
WDOG    equ 0x7000      ; read
PPI0    equ 0x8100      ; A8 picks the first PPI, A9 the second
PPI1    equ 0x8200
PSTEP   equ 1           ; register n at PPIx + n * PSTEP
    ENDIF

    IFDEF SCOBRA
RAM     equ 0x8000      ; mirrored at C000h
STACK   equ 0x8800
VRAM    equ 0x8800
OBJ     equ 0x9000
PPI0    equ 0x9800
PPI1    equ 0xA000
NMIEN   equ 0xA801
COIN    equ 0xA802
BGEN    equ 0xA803
STARS   equ 0xA804
FLIPX   equ 0xA806
FLIPY   equ 0xA807
WDOG    equ 0xB000
PSTEP   equ 1
    ENDIF

    IFDEF FROGGER
RAM     equ 0x8000
STACK   equ 0x8800
WDOG    equ 0x8800      ; read
VRAM    equ 0xA800
OBJ     equ 0xB000
NMIEN   equ 0xB808
FLIPY   equ 0xB80C
FLIPX   equ 0xB810
COIN    equ 0xB818
PPI0    equ 0xE000      ; A13 picks the first PPI, A12 the second, A1-A2 the register
PPI1    equ 0xD000
PSTEP   equ 2
STARS   equ RAM + 0x400 ; no stars on this board: a spare RAM byte, clear of the stack
BGEN    equ RAM + 0x401
    ENDIF

    IFDEF TURTLES
RAM     equ 0x8000
STACK   equ 0x8800
VRAM    equ 0x9000
OBJ     equ 0x9800
BGR     equ 0xA000
NMIEN   equ 0xA008
FLIPY   equ 0xA010
FLIPX   equ 0xA018
BGG     equ 0xA020
BGB     equ 0xA028
COIN    equ 0xA030
WDOG    equ 0xA800
PPI0    equ 0xB000      ; A4-A5 the register
PPI1    equ 0xB800
PSTEP   equ 16
STARS   equ RAM + 0x400
BGEN    equ RAM + 0x401
    ENDIF

PPI0A   equ PPI0
PPI0B   equ PPI0 + PSTEP
PPI0C   equ PPI0 + 2 * PSTEP
PPI0CTL equ PPI0 + 3 * PSTEP
PPI1A   equ PPI1
PPI1B   equ PPI1 + PSTEP
PPI1C   equ PPI1 + 2 * PSTEP
PPI1CTL equ PPI1 + 3 * PSTEP

SCROLL  equ OBJ         ; column n: scroll at 2n, colour at 2n+1
SPR     equ OBJ + 0x40  ; sprite n: y, code | xflip << 6 | yflip << 7, colour, x
SHOT    equ OBJ + 0x60  ; shot n: -, y, -, x

; Work RAM the frame loop uses
FRAMES  equ RAM         ; 16-bit count of NMIs
BOOTS   equ RAM + 2     ; warm boots (the watchdog's resets keep RAM)
MAGIC   equ RAM + 3
NOKICK  equ RAM + 5     ; non-zero: the NMI handler stops kicking the watchdog
CTRL    equ RAM + 6     ; what PPI1 port B holds
USER    equ RAM + 0x10
