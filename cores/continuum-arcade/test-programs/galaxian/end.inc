; The end of every Galaxian-board test program: the program image, 10 KB on Galaxian's five
; ROMs (0000h-27FFh) or 16 KB on Moon Cresta's eight. OUTNAME is set by the program.
    IFDEF MOONCRESTA
    SAVEBIN OUTNAME, 0, 0x4000
    ELSE
    ASSERT $ <= 0x2800
    SAVEBIN OUTNAME, 0, 0x2800
    ENDIF
