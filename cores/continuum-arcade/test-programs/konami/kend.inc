; The end of every Konami-board main test program: 12 KB (0000h-2FFFh), which fits every one of
; the boards' sets (The End has the fewest program ROMs). OUTNAME is set by the program.
    ASSERT $ <= 0x3000
    SAVEBIN OUTNAME, 0, 0x3000
