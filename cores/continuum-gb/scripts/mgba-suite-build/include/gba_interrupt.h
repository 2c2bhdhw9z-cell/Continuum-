// Minimal libgba-compatible interrupt header.
#ifndef GBA_INTERRUPT_H
#define GBA_INTERRUPT_H

#include <gba_base.h>

#define REG_IE     (*(vu16*)0x4000200)
#define REG_IF     (*(vu16*)0x4000202)
#define REG_WAITCNT (*(vu16*)0x4000204)
#define REG_IME    (*(vu16*)0x4000208)
#define REG_POSTFLG (*(vu8*)0x4000300)
#define REG_HALTCNT (*(vu8*)0x4000301)

#define IRQ_VBLANK 0x0001
#define IRQ_HBLANK 0x0002
#define IRQ_VCOUNT 0x0004
#define IRQ_TIMER0 0x0008
#define IRQ_TIMER1 0x0010
#define IRQ_TIMER2 0x0020
#define IRQ_TIMER3 0x0040
#define IRQ_SERIAL 0x0080
#define IRQ_DMA0   0x0100
#define IRQ_DMA1   0x0200
#define IRQ_DMA2   0x0400
#define IRQ_DMA3   0x0800
#define IRQ_KEYPAD 0x1000
#define IRQ_GAMEPAK 0x2000

// irqSet/irqEnable/irqDisable all take IRQ bit masks (IRQ_VBLANK | IRQ_HBLANK,
// ...), as in libgba: irqSet registers the handler for every set bit.
void irqInit(void);
void irqSet(int mask, void (*handler)(void));
void irqEnable(int mask);
void irqDisable(int mask);
void irqMasterEnable(void);
void irqMasterDisable(void);

// The IRQ entry point, installed by irqInit at 0x03007FFC (in IWRAM). The BIOS's
// IRQ vector jumps there; it acks the fired bits into the BIOS's acknowledged-flags
// word at 0x03007FF8 (which is what IntrWait/VBlankIntrWait poll), clears them in
// REG_IF, and calls the registered handlers.
void IntrMain(void);
// C dispatcher called by IntrMain once it has switched to System mode.
void irqDispatch(void);

#endif
