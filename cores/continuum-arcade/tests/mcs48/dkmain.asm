; The Z80 side of the 8035 trace tests (tests/mcs48_mame.rs): on MAME's Donkey Kong board it
; sets the sound board's inputs once and then idles, so the 8035 programs see them steady.
	cpu	z80
	org	0
	di
	ld	a,1
	ld	(7D80h),a	; the 8035's /INT, held asserted
	ld	(7D04h),a	; latch 6H bit 4: T1 reads low
	ld	(7D03h),a	; latch 6H bit 3: pulls P2 bit 5 low
	ld	a,5
	ld	(7C00h),a	; the sound command latch: 5, which the 8035 reads as 0Ah
idle:	jr	idle
