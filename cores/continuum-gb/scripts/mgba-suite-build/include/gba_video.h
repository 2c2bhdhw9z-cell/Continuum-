// Minimal libgba-compatible video header.
#ifndef GBA_VIDEO_H
#define GBA_VIDEO_H

#include <gba_base.h>

#define REG_DISPCNT  (*(vu16*)0x4000000)
#define REG_DISPSTAT (*(vu16*)0x4000004)
#define REG_VCOUNT   (*(vu16*)0x4000006)
#define REG_BG0CNT   (*(vu16*)0x4000008)
#define REG_BG1CNT   (*(vu16*)0x400000A)
#define REG_BG2CNT   (*(vu16*)0x400000C)
#define REG_BG3CNT   (*(vu16*)0x400000E)
#define REG_BG0HOFS  (*(vu16*)0x4000010)
#define REG_BG0VOFS  (*(vu16*)0x4000012)
#define REG_BG1HOFS  (*(vu16*)0x4000014)
#define REG_BG1VOFS  (*(vu16*)0x4000016)
#define REG_BG2HOFS  (*(vu16*)0x4000018)
#define REG_BG2VOFS  (*(vu16*)0x400001A)
#define REG_BG3HOFS  (*(vu16*)0x400001C)
#define REG_BG3VOFS  (*(vu16*)0x400001E)
#define REG_BG2PA    (*(vs16*)0x4000020)
#define REG_BG2PB    (*(vs16*)0x4000022)
#define REG_BG2PC    (*(vs16*)0x4000024)
#define REG_BG2PD    (*(vs16*)0x4000026)
#define REG_BG2X     (*(vs32*)0x4000028)
#define REG_BG2Y     (*(vs32*)0x400002C)
#define REG_BG3PA    (*(vs16*)0x4000030)
#define REG_BG3PB    (*(vs16*)0x4000032)
#define REG_BG3PC    (*(vs16*)0x4000034)
#define REG_BG3PD    (*(vs16*)0x4000036)
#define REG_BG3X     (*(vs32*)0x4000038)
#define REG_BG3Y     (*(vs32*)0x400003C)
#define REG_WIN0H    (*(vu16*)0x4000040)
#define REG_WIN1H    (*(vu16*)0x4000042)
#define REG_WIN0V    (*(vu16*)0x4000044)
#define REG_WIN1V    (*(vu16*)0x4000046)
#define REG_WININ    (*(vu16*)0x4000048)
#define REG_WINOUT   (*(vu16*)0x400004A)
#define REG_MOSAIC   (*(vu16*)0x400004C)
#define REG_BLDCNT   (*(vu16*)0x4000050)
#define REG_BLDALPHA (*(vu16*)0x4000052)
#define REG_BLDY     (*(vu16*)0x4000054)

#define MODE_0 0x0000
#define MODE_1 0x0001
#define MODE_2 0x0002
#define MODE_3 0x0003
#define MODE_4 0x0004
#define MODE_5 0x0005

#define BG0_ON   0x0100
#define BG1_ON   0x0200
#define BG2_ON   0x0400
#define BG3_ON   0x0800
#define OBJ_ON   0x1000
#define WIN0_ON  0x2000
#define WIN1_ON  0x4000
#define OBJ_WIN_ON 0x8000

// DISPSTAT: bits 0-2 are the flags, bits 3-5 the IRQ enables, bits 8-15 the
// VCount setting. LCDC_OFF is DISPCNT bit 7 (forced blank).
#define LCDC_VBL_FLAG 0x0001
#define LCDC_HBL_FLAG 0x0002
#define LCDC_VCNT_FLAG 0x0004
#define LCDC_VBL 0x0008
#define LCDC_HBL 0x0010
#define LCDC_VCNT 0x0020
#define LCDC_VCNT_SET 0x0100
#define VCOUNT(n) ((u16)(((n) & 0xFF) << 8))
#define LCDC_OFF 0x0080

#define VRAM       ((u16*)0x06000000)
#define BG_PALETTE ((u16*)0x05000000)
#define SPRITE_PALETTE ((u16*)0x05000200)
// MAP[screenblock][y][x]: VRAM as 32x32 tile maps of u16 entries.
#define MAP ((u16 (*)[32][32])0x06000000)

#define CHAR_BASE(n) ((n) << 2)
#define SCREEN_BASE(n) ((n) << 8)
#define TILE_BASE_ADR(n) ((u16*)(0x06000000 + ((n) << 14)))
#define SCREEN_BASE_BLOCK(n) ((u16*)(0x06000000 + ((n) << 11)))
#define MAP_BASE(n) SCREEN_BASE_BLOCK(n)
// SPR_VRAM(n) is the address of sprite tile n in 1D mapping (32 bytes a tile).
#define SPR_VRAM(n) ((void*)(0x06010000 + ((n) << 5)))

#define RGB5(r, g, b) ((u16)((r) | ((g) << 5) | ((b) << 10)))
#define RGB8(r, g, b) RGB5((r) >> 3, (g) >> 3, (b) >> 3)

#endif
