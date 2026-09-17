/*
 * Continuum GBA test cart — an original ROM written for this project.
 *
 * The GBA counterpart to `scripts/make-test-rom.py`. Same job: prove that a real core
 * is emulating, and be unambiguous about it under test.
 *
 * Deliberate design choices for verification:
 *
 *   - The idle screen is **blue**, where the NES cart's is **green**. A hot-swap test
 *     can therefore tell which core produced a frame from the pixels alone, with no
 *     bookkeeping to trust.
 *   - Holding **A** repaints the palette yellow, so input is visible as a whole-screen
 *     change.
 *   - **Left/Right** move a white marker block, which proves per-frame register and
 *     VRAM writes land (a palette change alone could be a one-off).
 *   - Channel 1 plays a continuous ~440 Hz square wave, so silence in the output means
 *     the audio path dropped it rather than the ROM never making any.
 *
 * Mode 4 (8bpp paletted, 240x160) is used rather than mode 3 so the per-frame work is
 * a couple of palette writes instead of a 38,400-pixel redraw.
 *
 * No Nintendo logo is included: mgba validates only the entry branch (byte 3 = 0xEA)
 * and the fixed 0x96 at offset 0xB2, both of which `make-gba-rom.sh` patches in. That
 * keeps this ROM entirely ours to distribute.
 */

typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;

#define REG(addr) (*(volatile u16*)(addr))

/* Display */
#define REG_DISPCNT REG(0x04000000)
#define REG_DISPSTAT REG(0x04000004)
#define REG_VCOUNT REG(0x04000006)
#define REG_KEYINPUT REG(0x04000130)

#define PALETTE ((volatile u16*)0x05000000)
#define VRAM ((volatile u16*)0x06000000)

/* Sound: DMG channel 1 plus the master/mix controls. */
#define REG_SOUND1CNT_L REG(0x04000060)
#define REG_SOUND1CNT_H REG(0x04000062)
#define REG_SOUND1CNT_X REG(0x04000064)
#define REG_SOUNDCNT_L REG(0x04000080)
#define REG_SOUNDCNT_H REG(0x04000082)
#define REG_SOUNDCNT_X REG(0x04000084)

#define SCREEN_W 240
#define SCREEN_H 160

/* Keys are active low. */
#define KEY_A 0x0001
#define KEY_B 0x0002
#define KEY_RIGHT 0x0010
#define KEY_LEFT 0x0020

/* BGR555. */
#define RGB(r, g, b) ((u16)(((b) << 10) | ((g) << 5) | (r)))
#define COLOUR_BLACK RGB(0, 0, 0)
#define COLOUR_BLUE RGB(2, 6, 31)
#define COLOUR_YELLOW RGB(31, 31, 2)
#define COLOUR_WHITE RGB(31, 31, 31)

/* Palette slots. Index 1 is the one the A button repaints. */
#define IDX_BG 0
#define IDX_FILL 1
#define IDX_MARKER 2

#define MARKER_SIZE 16
#define MARKER_Y 72

/*
 * Writes one 8bpp pixel pair. GBA VRAM ignores 8-bit writes, so mode 4 pixels must be
 * written in halfwords — a byte-at-a-time loop silently draws nothing, which is a
 * classic first-GBA-program bug.
 */
static inline void put_pair(int x, int y, u8 left, u8 right) {
   VRAM[(y * SCREEN_W + x) >> 1] = (u16)(left | (right << 8));
}

/* An 8x8 grid of filled blocks with a one-pixel border, matching the NES cart's look. */
static void draw_pattern(void) {
   for (int y = 0; y < SCREEN_H; y++) {
      int border_row = (y & 7) == 0 || (y & 7) == 7;
      for (int x = 0; x < SCREEN_W; x += 2) {
         u8 left = IDX_FILL;
         u8 right = IDX_FILL;
         if (border_row) {
            left = IDX_BG;
            right = IDX_BG;
         } else {
            if ((x & 7) == 0 || (x & 7) == 7) left = IDX_BG;
            if (((x + 1) & 7) == 0 || ((x + 1) & 7) == 7) right = IDX_BG;
         }
         put_pair(x, y, left, right);
      }
   }
}

/* Restores the pattern under the marker rather than clearing to a flat colour. */
static void draw_block(int x0, u8 index, int restore) {
   if (x0 < 0) x0 = 0;
   if (x0 > SCREEN_W - MARKER_SIZE) x0 = SCREEN_W - MARKER_SIZE;
   for (int y = MARKER_Y; y < MARKER_Y + MARKER_SIZE; y++) {
      int border_row = (y & 7) == 0 || (y & 7) == 7;
      for (int x = x0; x < x0 + MARKER_SIZE; x += 2) {
         if (restore) {
            u8 left = border_row || (x & 7) == 0 || (x & 7) == 7 ? IDX_BG : IDX_FILL;
            u8 right = border_row || ((x + 1) & 7) == 0 || ((x + 1) & 7) == 7 ? IDX_BG : IDX_FILL;
            put_pair(x, y, left, right);
         } else {
            put_pair(x, y, index, index);
         }
      }
   }
}

static void start_tone(void) {
   REG_SOUNDCNT_X = 0x0080; /* master enable */
   /* Full volume both sides, channel 1 routed left and right. */
   REG_SOUNDCNT_L = 0x1177;
   REG_SOUNDCNT_H = 0x0002; /* DMG mix at 100% */
   REG_SOUND1CNT_L = 0x0000; /* no sweep */
   /* 50% duty, envelope volume 15, no envelope decay. */
   REG_SOUND1CNT_H = 0xF080;
   /* rate = 2048 - 131072/440 ~= 1750; bit 15 restarts the channel. */
   REG_SOUND1CNT_X = 1750 | 0x8000;
}

static void wait_vblank(void) {
   /* Poll rather than use an interrupt: no vector table, no BIOS assumptions. */
   while (REG_VCOUNT >= SCREEN_H) {
   }
   while (REG_VCOUNT < SCREEN_H) {
   }
}

int main(void) {
   PALETTE[IDX_BG] = COLOUR_BLACK;
   PALETTE[IDX_FILL] = COLOUR_BLUE;
   PALETTE[IDX_MARKER] = COLOUR_WHITE;

   /* Mode 4 (8bpp bitmap), BG2 enabled. */
   REG_DISPCNT = 0x0404;

   draw_pattern();
   start_tone();

   int marker_x = (SCREEN_W - MARKER_SIZE) / 2;
   draw_block(marker_x, IDX_MARKER, 0);

   for (;;) {
      wait_vblank();

      u16 keys = ~REG_KEYINPUT; /* active low, so invert for "pressed" */

      /* A repaints the fill colour: one register write, whole-screen effect. */
      PALETTE[IDX_FILL] = (keys & KEY_A) ? COLOUR_YELLOW : COLOUR_BLUE;

      int next_x = marker_x;
      if (keys & KEY_RIGHT) next_x += 2;
      if (keys & KEY_LEFT) next_x -= 2;
      if (next_x < 0) next_x = 0;
      if (next_x > SCREEN_W - MARKER_SIZE) next_x = SCREEN_W - MARKER_SIZE;

      if (next_x != marker_x) {
         draw_block(marker_x, 0, 1); /* restore the pattern underneath */
         marker_x = next_x;
         draw_block(marker_x, IDX_MARKER, 0);
      }
   }
}
