; The end of every Donkey Kong-board test program: the program image, 16 KB on Donkey Kong's four
; ROMs (0000h-3FFFh), or 24 KB on Jr.'s three (0000h-5FFFh). OUTNAME is set by the program.
    IFDEF JUNIOR
    ASSERT $ <= 0x6000
    SAVEBIN OUTNAME, 0, 0x6000
    ELSE
    ASSERT $ <= 0x4000
    SAVEBIN OUTNAME, 0, 0x4000
    ENDIF
