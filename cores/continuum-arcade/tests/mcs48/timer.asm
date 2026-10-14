; 8035 trace test: the timer and its prescaler, JTF, the timer interrupt, an overflow with the
; interrupt off, overflows waiting inside the routine (and dropped by DIS TCNTI), the event
; counter with T1 held low, and the external interrupt taken before a waiting timer one.
	cpu	8048
	org	0
	jmp	start
	org	3
	jmp	xisr
	org	7
	jmp	tisr

	org	10h
start:	mov	r7,#0		; let the main CPU set the board's inputs first
wait:	djnz	r7,wait
	mov	a,#0FCh		; 1. polled from FCh: MOV A,T sees each count, JTF the overflow
	mov	t,a
	strt	t
poll1:	mov	a,t
	jtf	over1
	jmp	poll1
over1:	jtf	poll1		; the flag cleared itself
	mov	a,t
	mov	r3,#20		; 2. MOV T,A leaves the prescaler counting; STRT T clears it
p2:	djnz	r3,p2
	mov	a,#80h
	mov	t,a
	mov	r3,#20
p3:	djnz	r3,p3
	strt	t
	stop	tcnt
	mov	a,t
	mov	a,#0FEh		; 3. the timer interrupt
	mov	t,a
	mov	r5,#0
	mov	r4,#0
	en	tcnti
	strt	t
idle3:	mov	a,r5
	jz	idle3
	stop	tcnt
	dis	tcnti		; 4. an overflow with the interrupt off sets only the flag
	mov	a,#0FFh
	mov	t,a
	strt	t
idle4:	jtf	got4
	jmp	idle4
got4:	en	tcnti		; and nothing comes of it later
	nop
	nop
	stop	tcnt
	mov	r4,#1		; 5. an overflow inside the routine waits for RETR
	mov	r5,#0
	mov	a,#0FFh
	mov	t,a
	strt	t
idle5:	mov	a,r5
	xrl	a,#2
	jnz	idle5
	stop	tcnt
	mov	r4,#2		; 6. unless DIS TCNTI drops it
	mov	r5,#0
	mov	a,#0FFh
	mov	t,a
	strt	t
idle6:	mov	a,r5
	jz	idle6
	mov	r3,#100
p6:	djnz	r3,p6
	stop	tcnt
	clr	a		; 7. the event counter, T1 held low: no counts
	mov	t,a
	strt	cnt
	mov	r3,#50
p7:	djnz	r3,p7
	mov	a,t
	stop	tcnt
	mov	r4,#3		; 8. both interrupts waiting at once: 003h first
	mov	r5,#0
	mov	r6,#0
	mov	a,#0FFh
	mov	t,a
	en	tcnti
	strt	t
idle8:	mov	a,r6
	jz	idle8
	mov	r3,#10
p8:	djnz	r3,p8
	stop	tcnt
	dis	tcnti
	jmp	done

	org	100h
tisr:	mov	a,t
	inc	r5
	mov	a,r4
	jz	tret		; mode 0: count and return
	xrl	a,#1
	jz	tm1
	mov	a,r4
	xrl	a,#2
	jz	tm2
	mov	r4,#0		; mode 3: overflow again inside, and /INT enabled
	mov	a,#0FFh
	mov	t,a
	mov	r3,#25
tw3:	djnz	r3,tw3
	en	i
	retr
tm1:	mov	a,r5		; mode 1: the second time just returns
	xrl	a,#2
	jz	tret
	mov	a,#0FFh
	mov	t,a
	mov	r3,#25
tw1:	djnz	r3,tw1
	retr
tm2:	mov	a,#0FFh		; mode 2: overflow inside, then drop it
	mov	t,a
	mov	r3,#25
tw2:	djnz	r3,tw2
	dis	tcnti
tret:	retr

xisr:	dis	i
	inc	r6
	mov	a,psw
	retr

done:	jmp	done
