// Minimal libgba-compatible sprite header.
#ifndef GBA_SPRITES_H
#define GBA_SPRITES_H

#include <gba_base.h>

// OAM entry: attr0/attr1/attr2 plus the fourth halfword, which doubles as the
// rotation/scaling parameter block (libgba calls it `dummy` in the version the
// suite targets).
typedef struct {
    u16 attr0;
    u16 attr1;
    u16 attr2;
    s16 dummy;
} OBJATTR;

typedef struct {
    u16 fill0[3];
    s16 pa;
    u16 fill1[3];
    s16 pb;
    u16 fill2[3];
    s16 pc;
    u16 fill3[3];
    s16 pd;
} OBJROTSCALE;

#define OAM ((OBJATTR*)0x07000000)
#define OBJ ((OBJATTR*)0x07000000)

// Field builders (rvalues): the suite uses them as ATTR0_COLOR_16 | OBJ_Y(0) etc.
#define OBJ_Y(n) ((u16)((n) & 0x00FF))
#define OBJ_X(n) ((u16)((n) & 0x01FF))
#define OBJ_CHAR(n) ((u16)((n) & 0x03FF))
#define OBJ_PALETTE(n) ((u16)(((n) & 0x000F) << 12))
#define OBJ_ROT_SCALE(n) ((u16)(((n) & 0x001F) << 9))
#define OBJ_COLORS SPRITE_PALETTE

#define OBJ_DISABLE 0x0200
#define OBJ_1D_MAP 0x0040

#define ATTR0_SQUARE 0x0000
#define ATTR0_WIDE   0x4000
#define ATTR0_TALL   0x8000
#define ATTR0_4BPP   0x0000
#define ATTR0_COLOR_256 0x2000
#define ATTR0_8BPP   0x2000
#define ATTR0_COLOR_16 0x0000
#define ATTR0_ROTSCALE 0x0100
#define ATTR0_HIDE   0x0200
#define ATTR0_BLEND  0x0400
#define ATTR0_WINDOW 0x0800
#define ATTR0_MOSAIC 0x1000

#define ATTR1_SIZE_8   0x0000
#define ATTR1_SIZE_16  0x4000
#define ATTR1_SIZE_32  0x8000
#define ATTR1_SIZE_64  0xC000
#define ATTR1_HFLIP    0x1000
#define ATTR1_VFLIP    0x2000

#define ATTR2_PALETTE(n) OBJ_PALETTE(n)
#define ATTR2_PRIORITY(n) ((u16)(((n) & 0x0003) << 10))

#endif
