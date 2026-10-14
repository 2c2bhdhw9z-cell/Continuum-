; 8035 trace test: arithmetic and logic on every accumulator value with eight operands, flags
; and decimal adjust included. Each instruction's result shows in the trace (tests/mcs48_mame.rs).
	cpu	8048
	org	0
	jmp	start
	org	3
	retr
	org	7
	retr

	org	10h
start:	mov	r7,#0		; let the main CPU set the board's inputs first
wait:	djnz	r7,wait
	mov	r6,#0		; operand number, 0-7
nextop:	mov	a,r6
	movp3	a,@a		; the operand, from the table at 300h
	mov	r4,a
	mov	r0,#20h
	mov	@r0,a		; and a copy at 20h
	mov	r3,#0		; the accumulator value under test: 0, then 255 down to 1
loop:	mov	a,r3
	add	a,r4
	mov	r5,a
	da	a
	mov	a,r3
	addc	a,r4		; the carry the decimal adjust left
	da	a
	mov	a,r3
	add	a,@r0
	mov	a,r3
	addc	a,@r0
	mov	a,r3
	add	a,#99h
	addc	a,#66h
	da	a
	mov	a,r3
	anl	a,r4
	mov	a,r3
	orl	a,@r0
	mov	a,r3
	xrl	a,#5Ah
	mov	a,r3
	anl	a,#0C3h
	orl	a,r4
	xrl	a,@r0
	mov	a,r3
	rlc	a
	rrc	a
	rl	a
	rr	a
	swap	a
	cpl	a
	inc	a
	dec	a
	cpl	c
	rlc	a
	xch	a,r5
	xchd	a,@r0
	mov	a,psw
	jc	carry
	inc	r2
carry:	jz	zero
	inc	r1
zero:	jb7	bit7
	dec	r1
bit7:	da	a
	inc	@r0
	dec	r4
	inc	r4
	djnz	r3,loop
	inc	r6
	mov	a,r6
	xrl	a,#8
	jnz	nextop
done:	jmp	done

	org	300h
	db	00h, 01h, 09h, 0Fh, 66h, 80h, 99h, 0FFh
