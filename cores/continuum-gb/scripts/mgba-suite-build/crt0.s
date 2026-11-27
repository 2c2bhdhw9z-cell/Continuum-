@ GBA startup for building mgba-emu/suite with a stock arm-none-eabi-gcc.
@ Layout (see gba.ld): the ROM header occupies 0x08000000-0x080000C0, the real
@ GBA layout — logo at 0x04-0x9F, title/game code/checksum at 0xA0-0xBF — so
@ gbafix.py's 0x96 byte (0xB2) and header checksum (0xBD) land in header space,
@ not in code. The entry branch is the first word and code starts at 0xC0.
@ address stored at 0x03007FFC, installed by irqInit(); the mini-BIOS itself
@ (at 0x128 in the emulator's BIOS) pushes r0-r3/r12/lr on the IRQ stack and
@ returns through 0x138, so IntrMain below only has to return to its caller.

	.arm
	.section .init, "ax"
	.global _start
_start:
	b	rom_start
	@ 0x08000004-0x080000C0: ROM header space, filled in by gbafix.py.
	.space	0xC0 - 4

	.text
	.arm
	.global rom_start
rom_start:
	@ Mode stacks. IWRAM's top 0x100 bytes (0x03007F00-0x03007FFF) are the
	@ BIOS's own workspace (0x03007FF8 ack flags, 0x03007FFC IRQ handler), so
	@ the user stacks stop at 0x03007A00; .iwram data must fit below that.
	mov	r0, #0x12		@ IRQ
	msr	cpsr_c, r0
	ldr	sp, =__sp_irq
	mov	r0, #0x11		@ FIQ
	msr	cpsr_c, r0
	ldr	sp, =__sp_fiq
	mov	r0, #0x13		@ Supervisor
	msr	cpsr_c, r0
	ldr	sp, =__sp_svc
	mov	r0, #0x17		@ Abort
	msr	cpsr_c, r0
	ldr	sp, =__sp_abt
	mov	r0, #0x1B		@ Undefined
	msr	cpsr_c, r0
	ldr	sp, =__sp_und
	mov	r0, #0x1F		@ System (shares the user stack)
	msr	cpsr_c, r0
	ldr	sp, =__sp_usr

	@ Copy .iwram, .ewram and .data from ROM to RAM.
	ldr	r0, =__iwram_lma
	ldr	r1, =__iwram_start
	ldr	r2, =__iwram_end
	bl	copy_words
	ldr	r0, =__ewram_lma
	ldr	r1, =__ewram_start
	ldr	r2, =__ewram_end
	bl	copy_words
	ldr	r0, =__data_lma
	ldr	r1, =__data_start
	ldr	r2, =__data_end
	bl	copy_words

	@ Zero .bss.
	ldr	r0, =__bss_start
	ldr	r1, =__bss_end
	mov	r2, #0
zero_loop:
	cmp	r0, r1
	strlt	r2, [r0], #4
	blt	zero_loop

	@ main is Thumb; bx to it with the mode bit set (the symbol carries it).
	ldr	r0, =main
	bx	r0
hang:
	b	hang

copy_words:
	cmp	r1, r2
	ldrlt	r3, [r0], #4
	strlt	r3, [r1], #4
	blt	copy_words
	bx	lr

@ ---------------------------------------------------------------------------
@ IntrMain: the IRQ entry point, in IWRAM. Entered from the emulator's
@ mini-BIOS in IRQ mode, ARM, with lr = the mini-BIOS's return address (0x138)
@ and r0-r3/r12/lr already saved on the IRQ stack by the mini-BIOS. Switches to
@ System mode (IRQs still off), calls the C dispatcher on the user stack, and
@ returns to the mini-BIOS, which restores the registers and does the IRQ return.
	.section .iwram, "ax", %progbits
	.arm
	.global IntrMain
	.type IntrMain, %function
IntrMain:
	stmfd	sp!, {r0-r3, r12, lr}
	mrs	r0, cpsr
	bic	r0, r0, #0x1F
	orr	r0, r0, #0x1F		@ System mode
	orr	r0, r0, #0x80		@ keep IRQs off while dispatching
	msr	cpsr_c, r0
	stmfd	sp!, {lr}		@ save the user lr
	bl	irqDispatch
	ldmfd	sp!, {lr}
	mrs	r0, cpsr
	bic	r0, r0, #0x1F
	orr	r0, r0, #0x12		@ IRQ mode
	msr	cpsr_c, r0
	ldmfd	sp!, {r0-r3, r12, lr}
	mov	pc, lr			@ back to the mini-BIOS
	.size IntrMain, .-IntrMain
