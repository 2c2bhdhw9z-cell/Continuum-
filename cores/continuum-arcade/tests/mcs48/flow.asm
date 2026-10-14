; 8035 trace test: every conditional jump both ways, JMPP, MOVP (also from the last byte of a
; page), MOVP3, jumps whose address byte crosses a page, both memory banks, CALL and RET across
; them, the stack wrapping both ways, RETR restoring the PSW, every PSW value, both register
; banks, the undefined opcodes, RAM above 3Fh, and running off the top of bank 0 into 000h.
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
	mov	r0,#30h		; back here after running off the top of bank 0?
	mov	a,@r0
	xrl	a,#5Ah
	jz	wrapped
	clr	a
	clr	c
	jz	c1		; taken
	inc	r2
c1:	jnz	c2		; not
	inc	r2
c2:	jc	c3		; not
	inc	r2
c3:	jnc	c4		; taken
	inc	r2
c4:	cpl	c
	jc	c5		; taken
	inc	r2
c5:	jnc	c6		; not
	inc	r2
c6:	jf0	c7		; not
	inc	r2
c7:	cpl	f0
	jf0	c8		; taken
	inc	r2
c8:	jf1	c9		; not
	inc	r2
c9:	cpl	f1
	jf1	c10		; taken
	inc	r2
c10:	jt0	c11		; T0 is high: taken
	inc	r2
c11:	jnt0	c12		; not
	inc	r2
c12:	jt1	c13		; T1 is held low: not
	inc	r2
c13:	jnt1	c14		; taken
	inc	r2
c14:	jtf	c15		; the timer never ran: not
	inc	r2
c15:	jni	c16		; /INT is held: taken
	inc	r2
c16:	mov	a,#55h
	jb0	b0
	inc	r3
b0:	jb1	b1
	inc	r3
b1:	jb2	b2
	inc	r3
b2:	jb3	b3
	inc	r3
b3:	jb4	b4
	inc	r3
b4:	jb5	b5
	inc	r3
b5:	jb6	b6
	inc	r3
b6:	jb7	b7
	inc	r3
b7:	cpl	a
	jb0	b8
	inc	r3
b8:	jb7	b9
	inc	r3
b9:	jmp	blk2

	org	0E0h
wrapped:	mov	@r0,#0
	jmp	done
done:	jmp	done

	org	100h
blk2:	mov	a,#jtab & 0FFh	; JMPP through a table in this page
	jmpp	@a
jt1:	inc	r4
	mov	a,#(jtab + 1) & 0FFh
	jmpp	@a
jt2:	inc	r4
	mov	a,#mtab & 0FFh	; MOVP from this page
	movp	a,@a
	mov	a,#7		; MOVP3 from page 3
	movp3	a,@a
	mov	r0,#31h		; every PSW value, counted at 31h by R0 of either bank
	sel	rb1
	mov	r0,#31h
	sel	rb0
	mov	@r0,#0
psw:	mov	a,@r0
	mov	psw,a
	mov	a,psw
	inc	@r0
	mov	a,@r0
	jnz	psw
	clr	a
	mov	psw,a
	mov	r5,#11h		; the register banks
	sel	rb1
	mov	r5,#22h
	mov	a,r5
	sel	rb0
	add	a,r5
	jmp	blk3
jtab:	db	jt1 & 0FFh, jt2 & 0FFh
mtab:	db	0A5h

	org	1F8h
blk3:	mov	a,#0C3h
	nop
	nop
	nop
	nop
	nop
	movp	a,@a		; at 1FFh: reads 2C3h, from the next page
	clr	a		; 200h
	jmp	jz1
	org	2C3h
	db	77h
	org	2F0h
pg2:	jmp	jz2
	org	2FEh
jz1:	jz	pg2		; at 2FEh: its address byte is at 2FFh, so it stays in page 2

	org	300h
	db	10h, 21h, 32h, 43h, 54h, 65h, 76h, 87h
	org	3FFh
jz2:	db	0C6h		; JZ at 3FFh: its address byte is at 400h, so it jumps in page 4
	db	pg4 & 0FFh

	org	410h
pg4:	sel	mb1
	nop
	jmp	inb1		; to bank 1 (the same ROM again on this board)
	org	440h
inb1:	sel	mb0
	call	sub0		; from bank 1 into bank 0
	sel	mb1
	call	sub0		; and within bank 1
	sel	mb0
	jmp	stack
sub0:	inc	r6
	ret

	org	480h
stack:	mov	a,#07h		; SP 7: the CALL uses the pair at 16h-17h and SP wraps to 0
	mov	psw,a
	call	sp7
	mov	r0,#16h
	mov	@r0,#ret0 & 0FFh
	inc	r0
	mov	@r0,#(ret0 >> 8) | 0A0h
	clr	a
	mov	psw,a
	ret			; SP 0: pops 16h-17h (and leaves the PSW)
ret0:	mov	a,psw
	mov	r0,#14h
	mov	@r0,#ret1 & 0FFh
	inc	r0
	mov	@r0,#(ret1 >> 8) | 0D0h
	retr			; SP 7: pops 14h-15h, CY, AC and BS with it
ret1:	mov	a,psw
	clr	a
	mov	psw,a
	jmp	misc
sp7:	ret

	org	500h
misc:	db	01h, 06h, 0Bh, 22h, 33h, 38h, 3Bh, 63h, 66h, 73h, 82h, 87h, 8Bh
	db	9Bh, 0A2h, 0A6h, 0B7h, 0C0h, 0C1h, 0C2h, 0C3h, 0D6h, 0E0h, 0E1h, 0E2h, 0F3h
	mov	r0,#45h		; this part's 64 bytes repeat above 3Fh
	mov	@r0,#77h
	mov	r1,#05h
	mov	a,@r1
	mov	r1,#0FFh
	inc	@r1
	mov	a,@r1
	xch	a,@r1
	xchd	a,@r1
	mov	a,@r1
	dec	r1
	clr	c
	call	flags		; and RETR brings CY, F0 and the bank back
	mov	a,psw
	ent0	clk
	mov	r0,#30h		; last, run off the top of bank 0
	mov	@r0,#5Ah
	jmp	top
flags:	cpl	c
	cpl	f0
	sel	rb1
	mov	a,psw
	retr

	org	7FFh
top:	nop			; the next fetch is from 000h
