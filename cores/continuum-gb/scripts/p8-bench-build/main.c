// Heavy GBA benchmark ROM for P8: Mode 2 (affine BG2 + BG3), alpha blending, and
// 128 sprites, with the affine matrices scrolling every frame. No input, no IRQs;
// it just draws the heaviest picture the PPU can be asked for and loops. Built with
// a stock arm-none-eabi-gcc from scripts/p8-bench-build/build.sh (same crt0/linker
// script/gbafix as the mgba-suite build).
typedef unsigned short u16;
typedef unsigned int u32;
typedef volatile unsigned char vu8;
typedef volatile unsigned short vu16;
typedef volatile signed int vs32;
typedef volatile short vs16;

#define REG_DISPCNT (*(vu16*)0x04000000)
#define REG_BG2CNT  (*(vu16*)0x0400000C)
#define REG_BG3CNT  (*(vu16*)0x0400000E)
#define REG_BG2PA   (*(vs16*)0x04000020)
#define REG_BG2PB   (*(vs16*)0x04000022)
#define REG_BG2PC   (*(vs16*)0x04000024)
#define REG_BG2PD   (*(vs16*)0x04000026)
#define REG_BG2X    (*(vs32*)0x04000028)
#define REG_BG2Y    (*(vs32*)0x0400002C)
#define REG_BG3PA   (*(vs16*)0x04000030)
#define REG_BG3PB   (*(vs16*)0x04000032)
#define REG_BG3PC   (*(vs16*)0x04000034)
#define REG_BG3PD   (*(vs16*)0x04000036)
#define REG_BG3X    (*(vs32*)0x04000038)
#define REG_BG3Y    (*(vs32*)0x0400003C)
#define REG_BLDCNT  (*(vu16*)0x04000050)
#define REG_BLDALPHA (*(vu16*)0x04000052)
#define REG_VCOUNT  (*(vu16*)0x04000006)

#define VRAM ((vu8*)0x06000000)
#define PAL  ((vu16*)0x05000000)
#define OAM  ((vu16*)0x07000000)

static void wait_vblank(void) {
    while (REG_VCOUNT < 160) {}
    while (REG_VCOUNT >= 160) {}
}

int main(void) {
    // 256-entry BG palette: a colour ramp, so the affine fetch is never uniform.
    for (int i = 0; i < 256; i++) {
        int r = i & 31, g = (i >> 2) & 31, b = (i >> 4) & 31;
        PAL[i] = (u16)(r | (g << 5) | (b << 10));
    }
    // OBJ palette: a different ramp.
    for (int i = 0; i < 256; i++) {
        int r = 31 - (i & 31), g = (i >> 1) & 31, b = 31 - ((i >> 3) & 31);
        PAL[256 + i] = (u16)(r | (g << 5) | (b << 10));
    }
    // BG tile data, char block 0 (0x0000-0x3FFF): 256 8bpp tiles, each a diagonal
    // gradient, so neighbouring pixels differ and the affine fetch cannot be
    // short-circuited. Each tile is 64 bytes.
    for (int t = 0; t < 256; t++) {
        for (int p = 0; p < 64; p++) {
            int px = p & 7, py = p >> 3;
            VRAM[t * 64 + p] = (vu8)((t + px * 3 + py * 5) & 0xFF);
        }
    }
    // OBJ tile data, at 0x10000: the same gradient, so the sprites are not blank.
    for (int t = 0; t < 256; t++) {
        for (int p = 0; p < 64; p++) {
            int px = p & 7, py = p >> 3;
            VRAM[0x10000 + t * 64 + p] = (vu8)((t * 2 + px * 5 + py * 3) & 0xFF);
        }
    }
    // Affine maps are 32x32 bytes. BG2 map at screen block 8 (0x4000), BG3 map at
    // screen block 9 (0x4800) — both clear of the 16K of tile data.
    for (int y = 0; y < 32; y++) {
        for (int x = 0; x < 32; x++) {
            VRAM[0x4000 + y * 32 + x] = (vu8)((x * 7 + y * 13) & 0xFF);
            VRAM[0x4800 + y * 32 + x] = (vu8)((x * 11 + y * 5 + 64) & 0xFF);
        }
    }
    // 128 sprites, 64x64, 8bpp, spread over the screen, half of them affine.
    for (int i = 0; i < 128; i++) {
        int x = (i * 37) % 200, y = (i * 53) % 120;
        int affine = (i & 1) ? 1 : 0;
        int g = (i / 2) & 31; // rotation group (32 groups, shared by two sprites)
        // attr0: Y, 8bpp (0x2000), affine flag (0x0100), square shape (00).
        OAM[i * 4 + 0] = (u16)(y | 0x2000 | (affine ? 0x0100 : 0));
        // attr1: X, size 64x64 (0xC000), rotation group for affine sprites.
        OAM[i * 4 + 1] = (u16)(x | 0xC000 | (affine ? (g << 9) : 0));
        // attr2: tile, 8bpp (0x2000).
        OAM[i * 4 + 2] = (u16)(((i * 3) & 0xFF) | 0x2000);
        OAM[i * 4 + 3] = 0;
        if (affine) {
            // Rotation params live in OAM at word 3/7/11/15 of the group (32 bytes).
            OAM[g * 16 + 3] = (u16)(256 + ((g * 7) & 0xFF) - 128);
            OAM[g * 16 + 7] = (u16)(((g * 13) & 0x7F) - 64);
            OAM[g * 16 + 11] = (u16)(((g * 11) & 0x7F) - 64);
            OAM[g * 16 + 15] = (u16)(256 + ((g * 5) & 0xFF) - 128);
        }
    }
    // BG2: char block 0, screen block 8. BG3: char block 0, screen block 9.
    REG_BG2CNT = 0x0800;
    REG_BG3CNT = 0x0900;
    // Alpha blend: BG2 over BG3 and objects, 50/50. First target BG2 (bit 2), mode
    // alpha (bit 6); second targets BG3 (bit 11) and OBJ (bit 12).
    REG_BLDCNT = 0x1C44;
    REG_BLDALPHA = 0x0808;
    REG_BG2X = 0;
    REG_BG2Y = 0;
    REG_BG3X = 0;
    REG_BG3Y = 0;
    REG_DISPCNT = 0x0002 | 0x0400 | 0x0800 | 0x1000; // Mode 2, BG2+BG3+OBJ on
    int frame = 0;
    for (;;) {
        wait_vblank();
        frame++;
        // Move the affine matrices every frame so the picture never settles.
        REG_BG2PA = (short)(256 + ((frame * 3) & 127) - 64);
        REG_BG2PB = (short)(((frame * 5) & 63) - 32);
        REG_BG2PC = (short)(((frame * 7) & 63) - 32);
        REG_BG2PD = (short)(256 + ((frame * 11) & 127) - 64);
        REG_BG2X = frame * 64;
        REG_BG2Y = frame * 32;
        REG_BG3PA = (short)(256 - ((frame * 2) & 63));
        REG_BG3PB = (short)(((frame * 3) & 31) - 16);
        REG_BG3PC = (short)(((frame * 9) & 31) - 16);
        REG_BG3PD = (short)(256 - ((frame * 4) & 63));
        REG_BG3X = -frame * 48;
        REG_BG3Y = frame * 16;
    }
    return 0;
}
