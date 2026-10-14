; The AY-3-8910 comparison (tests/ay8910_mame.rs): a Z80 program for Jump Bug's board, whose
; AY-3-8910 (at 5800h data, 5900h address) MAME clocks at 1.536 MHz, so its output runs at
; 192,000 samples a second. It works down the script below: latch a register, write it, wait.
; Every channel plays alone (the others at amplitude 0), so one level shows at a time.
	cpu	z80

; A script entry: register, value, then how long to wait, in rounds of 26 T-states.
ent	macro	r,v,w
	db	r, v, (w) & 0FFh, (w) >> 8
	endm

	org	0
	di
	ld	sp,4800h
	ld	de,5900h	; the address latch
	ld	hl,5800h	; the data register
	ld	ix,script
next:	ld	a,(ix+0)
	cp	0FFh
	jr	z,done
	ld	(de),a
	ld	a,(ix+1)
	ld	(hl),a
	ld	c,(ix+2)
	ld	b,(ix+3)
wait:	dec	bc
	ld	a,b
	or	c
	jr	nz,wait
	inc	ix
	inc	ix
	inc	ix
	inc	ix
	jr	next
done:	jr	done

script:
	; The calibration, with the mixer off so each level holds steady: every fixed amplitude on
	; A, then the envelope's sixteen levels rising (shape 13, EP 64: 128 samples a level),
	; which MAME's output stage gives slightly different voltages.
	ent	7, 3Fh, 1
	irp	v, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0
	ent	8, v, 100
	endm
	ent	11, 64, 1
	ent	8, 10h, 1
	ent	13, 0Dh, 1400
	ent	8, 0, 1
	ent	11, 0, 1
	irp	v, 15, 7, 1, 0
	ent	9, v, 100
	endm
	irp	v, 14, 6, 2, 0
	ent	10, v, 100
	endm

	; Tone A at several periods, 0 (as 1) up to 4095, and periods changed mid-wave.
	ent	7, 3Eh, 1
	ent	8, 15, 1
	ent	0, 1, 30
	ent	0, 0, 30
	ent	0, 2, 40
	ent	0, 3, 60
	ent	0, 100, 500
	ent	0, 200, 100	; lowered below the count: it wraps at once
	ent	0, 10, 200
	ent	0, 250, 300	; raised: the count carries on to the new period
	ent	1, 1, 400	; 1FAh
	ent	1, 0F1h, 300	; the coarse register's top bits are not there
	ent	0, 23h, 600	; 123h
	ent	1, 0Fh, 1
	ent	0, 0FFh, 7000	; 4095
	ent	1, 0, 1
	ent	0, 7, 200
	ent	8, 0, 1

	; Tone B and tone C.
	ent	7, 3Dh, 1
	ent	9, 12, 1
	ent	2, 5, 200
	ent	3, 2, 400	; 205h
	ent	3, 0, 1
	ent	2, 1, 40
	ent	9, 0, 1
	ent	7, 3Bh, 1
	ent	10, 9, 1
	ent	4, 13, 300
	ent	5, 1, 300	; 10Dh
	ent	5, 0, 1
	ent	10, 0, 1

	; Noise on A at several periods (the top three bits of R6 are not there), then with tone A.
	ent	7, 37h, 1
	ent	8, 15, 1
	ent	6, 0, 400
	ent	6, 1, 400
	ent	6, 2, 400
	ent	6, 5, 600
	ent	6, 0E3h, 600	; 3
	ent	6, 31, 1500
	ent	7, 36h, 1
	ent	0, 7, 1
	ent	6, 3, 800
	ent	8, 0, 1

	; Noise on B and on C: the one noise generator, through each channel's mixer.
	ent	7, 2Fh, 1
	ent	9, 11, 600
	ent	9, 0, 1
	ent	7, 1Fh, 1
	ent	10, 13, 600
	ent	10, 0, 1

	; The envelope on A with the mixer off: all sixteen shapes, EP 2 (four samples a step).
	ent	7, 3Fh, 1
	ent	11, 2, 1
	ent	12, 0, 1
	ent	8, 10h, 1
	irp	s, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
	ent	13, s, 220
	endm
	; EP 0 (half of EP 1), EP 1, EP 300; the shape rewritten mid-cycle; the period changed
	; mid-step; and the amplitude's M bit off and on (its low bits don't matter with M set).
	ent	11, 0, 1
	ent	13, 0Eh, 150
	ent	11, 1, 1
	ent	13, 0Ah, 200
	ent	13, 0Ah, 50
	ent	13, 0Ah, 100
	ent	11, 2Ch, 1
	ent	12, 1, 1
	ent	13, 08h, 1500
	ent	11, 3, 1
	ent	12, 0, 1
	ent	13, 0Ch, 70
	ent	11, 9, 300
	ent	8, 0Fh, 50
	ent	8, 1Fh, 100
	ent	8, 13h, 100

	; The envelope through the tone on A, then on B and C.
	ent	7, 3Eh, 1
	ent	0, 3, 1
	ent	11, 1, 1
	ent	13, 0Eh, 400
	ent	8, 0, 1
	ent	7, 3Dh, 1
	ent	2, 4, 1
	ent	9, 10h, 1
	ent	13, 0Ch, 300
	ent	9, 0, 1
	ent	7, 3Bh, 1
	ent	4, 6, 1
	ent	10, 10h, 1
	ent	13, 0Ah, 300
	ent	10, 0, 1

	; And quiet to the end.
	ent	7, 3Fh, 1
	db	0FFh
