// IRQ registration and dispatch. The BIOS's own IRQ vector jumps to IntrMain (in
// IWRAM, installed by irqInit at 0x03007FFC). IntrMain switches to System mode and
// calls irqDispatch here.
//
// The dispatch mirrors what the GBA BIOS itself does on every IRQ: the fired bits
// (IE & IF) are ORed into the BIOS's acknowledged-interrupt word at 0x03007FF8 —
// that is what IntrWait/VBlankIntrWait poll — and written back to REG_IF to
// acknowledge them, then the registered handlers run. Bits with no handler are still
// acked, so a VBlank with no handler registered still wakes VBlankIntrWait.
//
// The register side effects follow real libgba (devkitPro libgba src/interrupt.c):
// irqInit does NOT touch IE or IF (it only installs the handler table and the
// vector) — an earlier version of this file zeroed them, which broke the suite:
// main's irqEnable(IRQ_VBLANK) would then be the only IE write, and tests that call
// irqInit() before VBlankIntrWait (e.g. the Misc suite's hblankBit) hung with IE=0.
// irqEnable/irqDisable also set/clear the matching DISPSTAT IRQ-enable bits
// (VBlank bit 3, HBlank bit 4, VCount bit 5) around the IE change, with IME off
// while they do it. The emulator gates the LCD IF flags on those DISPSTAT bits,
// so without them the suite's menu (VBlankIntrWait) and DMA tests (IntrWait on
// HBlank) hang. Do not "simplify" these away.
#include <gba_interrupt.h>
#include <gba_video.h>

static void (*irqTable[14])(void);

void irqInit(void) {
    for (int i = 0; i < 14; i++) {
        irqTable[i] = 0;
    }
    *(volatile u32*)0x03007FFC = (u32)IntrMain;
}

void irqSet(int mask, void (*handler)(void)) {
    for (int i = 0; i < 14; i++) {
        if (mask & (1 << i)) {
            irqTable[i] = handler;
        }
    }
}

void irqEnable(int mask) {
    REG_IME = 0;
    if (mask & IRQ_VBLANK) REG_DISPSTAT |= LCDC_VBL;
    if (mask & IRQ_HBLANK) REG_DISPSTAT |= LCDC_HBL;
    if (mask & IRQ_VCOUNT) REG_DISPSTAT |= LCDC_VCNT;
    REG_IE |= (u16)mask;
    REG_IME = 1;
}

void irqDisable(int mask) {
    REG_IME = 0;
    if (mask & IRQ_VBLANK) REG_DISPSTAT &= (u16)~LCDC_VBL;
    if (mask & IRQ_HBLANK) REG_DISPSTAT &= (u16)~LCDC_HBL;
    if (mask & IRQ_VCOUNT) REG_DISPSTAT &= (u16)~LCDC_VCNT;
    REG_IE &= (u16)~mask;
    REG_IME = 1;
}

void irqMasterEnable(void) {
    REG_IME = 1;
}

void irqMasterDisable(void) {
    REG_IME = 0;
}

void irqDispatch(void) {
    u16 pending = REG_IE & REG_IF;
    if (!pending) {
        return;
    }
    *(volatile u16*)0x03007FF8 |= pending;
    REG_IF = pending;
    for (int i = 0; i < 14; i++) {
        if ((pending & (1 << i)) && irqTable[i]) {
            irqTable[i]();
        }
    }
}
