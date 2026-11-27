// Minimal libgba-compatible timer header.
#ifndef GBA_TIMERS_H
#define GBA_TIMERS_H

#include <gba_base.h>

#define REG_TM0CNT_L (*(vu16*)0x4000100)
#define REG_TM0CNT_H (*(vu16*)0x4000102)
#define REG_TM0CNT   (*(vu32*)0x4000100)
#define REG_TM1CNT_L (*(vu16*)0x4000104)
#define REG_TM1CNT_H (*(vu16*)0x4000106)
#define REG_TM1CNT   (*(vu32*)0x4000104)
#define REG_TM2CNT_L (*(vu16*)0x4000108)
#define REG_TM2CNT_H (*(vu16*)0x400010A)
#define REG_TM2CNT   (*(vu32*)0x4000108)
#define REG_TM3CNT_L (*(vu16*)0x400010C)
#define REG_TM3CNT_H (*(vu16*)0x400010E)
#define REG_TM3CNT   (*(vu32*)0x400010C)

#define TIMER_START 0x0080
#define TIMER_IRQ   0x0040
#define TIMER_CASCADE 0x0004
#define TIMER_1CLK  0x0000
#define TIMER_64CLK 0x0001
#define TIMER_256CLK 0x0002
#define TIMER_1024CLK 0x0003

#endif
