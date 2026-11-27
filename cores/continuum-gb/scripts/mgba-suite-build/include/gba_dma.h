// Minimal libgba-compatible DMA header.
#ifndef GBA_DMA_H
#define GBA_DMA_H

#include <gba_base.h>

#define REG_DMA0SAD (*(vu32*)0x40000B0)
#define REG_DMA0DAD (*(vu32*)0x40000B4)
#define REG_DMA0CNT (*(vu32*)0x40000B8)
#define REG_DMA0CNT_L (*(vu16*)0x40000B8)
#define REG_DMA0CNT_H (*(vu16*)0x40000BA)
#define REG_DMA1SAD (*(vu32*)0x40000BC)
#define REG_DMA1DAD (*(vu32*)0x40000C0)
#define REG_DMA1CNT (*(vu32*)0x40000C4)
#define REG_DMA1CNT_L (*(vu16*)0x40000C4)
#define REG_DMA1CNT_H (*(vu16*)0x40000C6)
#define REG_DMA2SAD (*(vu32*)0x40000C8)
#define REG_DMA2DAD (*(vu32*)0x40000CC)
#define REG_DMA2CNT (*(vu32*)0x40000D0)
#define REG_DMA2CNT_L (*(vu16*)0x40000D0)
#define REG_DMA2CNT_H (*(vu16*)0x40000D2)
#define REG_DMA3SAD (*(vu32*)0x40000D4)
#define REG_DMA3DAD (*(vu32*)0x40000D8)
#define REG_DMA3CNT (*(vu32*)0x40000DC)
#define REG_DMA3CNT_L (*(vu16*)0x40000DC)
#define REG_DMA3CNT_H (*(vu16*)0x40000DE)

typedef struct {
    vu32 SAD;
    vu32 DAD;
    vu32 CNT;
} DMA_CONTROLLER;

#define DMA ((volatile DMA_CONTROLLER*)0x40000B0)
#define DMA0 ((volatile DMA_CONTROLLER*)0x40000B0)
#define DMA1 ((volatile DMA_CONTROLLER*)0x40000BC)
#define DMA2 ((volatile DMA_CONTROLLER*)0x40000C8)
#define DMA3 ((volatile DMA_CONTROLLER*)0x40000D4)

#define DMA16 0x00000000
#define DMA32 0x04000000
#define DMA_IMMEDIATE 0x00000000
#define DMA_VBLANK 0x10000000
#define DMA_HBLANK 0x20000000
#define DMA_SPECIAL 0x30000000
#define DMA_REPEAT 0x02000000
#define DMA_DST_INC 0x00000000
#define DMA_DST_DEC 0x00200000
#define DMA_DST_FIXED 0x00400000
#define DMA_DST_RELOAD 0x00600000
#define DMA_SRC_INC 0x00000000
#define DMA_SRC_DEC 0x00800000
#define DMA_SRC_FIXED 0x01000000
#define DMA_IRQ 0x40000000
#define DMA_ENABLE 0x80000000

// libgba's copy macros: count is in halfwords (DMA16) or words (DMA32).
#define DMA0COPY(src, dst, count) do { REG_DMA0SAD = (u32)(src); REG_DMA0DAD = (u32)(dst); REG_DMA0CNT = (count) | DMA_ENABLE; } while (0)
#define DMA1COPY(src, dst, count) do { REG_DMA1SAD = (u32)(src); REG_DMA1DAD = (u32)(dst); REG_DMA1CNT = (count) | DMA_ENABLE; } while (0)
#define DMA2COPY(src, dst, count) do { REG_DMA2SAD = (u32)(src); REG_DMA2DAD = (u32)(dst); REG_DMA2CNT = (count) | DMA_ENABLE; } while (0)
#define DMA3COPY(src, dst, count) do { REG_DMA3SAD = (u32)(src); REG_DMA3DAD = (u32)(dst); REG_DMA3CNT = (count) | DMA_ENABLE; } while (0)

#endif
