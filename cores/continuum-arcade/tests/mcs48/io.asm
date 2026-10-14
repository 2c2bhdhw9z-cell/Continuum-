; 8035 trace test: the outside world as Donkey Kong's sound board wires it. Port 1 (the DAC),
; port 2 (whose bit 5 the board pulls low, and whose bits 0-2 and 6 pick what MOVX reads), MOVX
; from every page of the tune ROM and from the sound command latch, the BUS, the 8243 expander
; instructions, T0 and T1, ENT0 CLK, and the external interrupt taken from bank 1.
	cpu	8048
	org	0
	jmp	start
	org	3
	jmp	xisr
	org	7
	retr

	org	10h
start:	mov	r7,#0		; let the main CPU set the board's inputs first
wait:	djnz	r7,wait
	mov	a,#5Ah		; port 1: nothing drives it, so it reads back its latch
	outl	p1,a
	anl	p1,#0F0h
	orl	p1,#03h
	in	a,p1
	mov	a,#0BFh		; port 2: the board pulls bit 5 low
	outl	p2,a
	in	a,p2
	anl	p2,#0FEh
	orl	p2,#01h
	in	a,p2
	mov	r3,#8		; MOVX from each 256-byte page of the tune ROM
pages:	mov	a,r3
	dec	a
	orl	a,#0B8h		; bit 6 clear, the page in bits 0-2
	outl	p2,a
	mov	r0,#0
	mov	r1,#80h
	mov	r2,#4
bytes:	movx	a,@r0
	movx	a,@r1
	inc	r0
	dec	r1
	movx	@r0,a		; writes go to the speech board, not fitted
	djnz	r2,bytes
	ins	a,bus		; the BUS reads the same, at address 0
	djnz	r3,pages
	orl	p2,#40h		; bit 6 set: MOVX reads the sound command latch
	movx	a,@r0
	ins	a,bus
	mov	a,#33h		; the BUS port
	outl	bus,a
	anl	bus,#0Fh
	orl	bus,#80h
	mov	a,#09h		; the 8243 instructions, through P2
	movd	p4,a
	movd	p7,a
	anld	p5,a
	orld	p6,a
	movd	a,p4
	movd	a,p7
	in	a,p2
	jt0	t0h		; T0 is high, T1 held low
	inc	r4
t0h:	jnt0	t0l
	inc	r4
t0l:	jt1	t1h
	inc	r4
t1h:	jnt1	t1l
	inc	r4
t1l:	ent0	clk
	jni	int1		; /INT is held: JNI sees it
	inc	r4
int1:	sel	mb1
	jmp	inb1

	org	200h
inb1:	en	i		; runs at A00h, in bank 1, and is interrupted at once
	nop
	sel	mb0
	jmp	done
done:	jmp	done

	org	300h
xisr:	sel	mb1		; inside the routine JMP and CALL stay in bank 0
	jmp	x2
x2:	call	xsub
	dis	i
	mov	a,psw
	retr			; back to bank 1
xsub:	in	a,p2
	ret
