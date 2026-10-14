; The end of every Pac-Man board test program: the mode 2 table entry for vector FEh, and the
; 16 KB program image (0000h-3FFFh). OUTNAME is set by the program.
    org 0x3FFE
    dw vblank
    SAVEBIN OUTNAME, 0, 0x4000
