// Measurement harness for cBoy's exact drawer: runs a ROM on SameBoy's core (MIT; fetched and
// built by build.sh, never committed to Continuum), stops a few frames after LD B,B, and writes
// the screen as raw RGB24 (160x144). With SB_LOG=1 it also prints SameBoy's PPU state at every
// write to FF40-FF4B during the frame after LD B,B, which compare-landings.py lines up against
// cBoy's (the `trace` feature) write by write.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "Core/gb.h"

static uint32_t screen[160 * 144];
static int vblanks = 0, done_at = -1, log_frame = -1;
static int want_log = 0;

static uint32_t rgb_encode(GB_gameboy_t *gb, uint8_t r, uint8_t g, uint8_t b) {
    return (uint32_t)r << 16 | (uint32_t)g << 8 | b;
}

static void vblank(GB_gameboy_t *gb, GB_vblank_type_t type) {
    vblanks++;
}

static void exec_cb(GB_gameboy_t *gb, uint16_t addr, uint8_t opcode) {
    if (want_log && vblanks == log_frame && (addr == 0x48 || addr == 0x40)) {
        GB_display_sync(gb);
        printf("X %04X line=%3d LY=%3d STAT=%02X state=%2d oam_idx=%2d dcyc=%d cyc=%d\n", addr, gb->current_line,
               gb->io_registers[GB_IO_LY], gb->io_registers[GB_IO_STAT], gb->display_state,
               gb->oam_search_index, gb->display_cycles, gb->cycles_for_line);
    }
    if (opcode == 0x40 && done_at < 0) {
        done_at = vblanks;
        log_frame = vblanks + 1;
    }
}

static int reads_logged = 0;
static uint8_t read_cb(GB_gameboy_t *gb, uint16_t addr, uint8_t data) {
    if (getenv("SB_READS") && addr == 0xFF41 && reads_logged < 100000) {
        GB_display_sync(gb);
        reads_logged++;
        printf("R %04X pc=%04X line=%3d state=%2d oam_idx=%2d dcyc=%d cyc=%d stat=%02X\n", addr, gb->pc, gb->current_line,
               gb->display_state, gb->oam_search_index, gb->display_cycles, gb->cycles_for_line,
               gb->io_registers[GB_IO_STAT]);
    }
    return data;
}

static bool write_cb(GB_gameboy_t *gb, uint16_t addr, uint8_t value) {
    if (!want_log || vblanks != log_frame) return true;
    if (addr < 0xFF40 || addr > 0xFF4B) return true;
    GB_display_sync(gb);
    if (gb->current_line >= 144) return true;
    printf("W %04X=%02X line=%3d pos=%4d lcd_x=%3d fetch=%d state=%2d cyc=%3d objs=%d during_obj=%d wx_trig=%d bgfifo=%d old=%02X\n",
           addr, value, gb->current_line, (int8_t)gb->position_in_line, gb->lcd_x, gb->fetcher_state,
           gb->display_state, gb->cycles_for_line, gb->n_visible_objs, gb->during_object_fetch,
           gb->wx_triggered, gb->bg_fifo.size, gb->io_registers[addr & 0x7F]);
    return true;
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: %s dmg|cgbc|cgbe boot.bin rom.gb out.rgb\n", argv[0]);
        return 1;
    }
    want_log = getenv("SB_LOG") != NULL;
    GB_model_t model = GB_MODEL_DMG_B;
    if (!strcmp(argv[1], "cgbc")) model = GB_MODEL_CGB_C;
    if (!strcmp(argv[1], "cgbd")) model = GB_MODEL_CGB_D;
    if (!strcmp(argv[1], "cgbe")) model = GB_MODEL_CGB_E;
    static GB_gameboy_t gb;
    GB_init(&gb, model);
    if (GB_load_boot_rom(&gb, argv[2])) { fprintf(stderr, "boot rom\n"); return 1; }
    if (GB_load_rom(&gb, argv[3])) { fprintf(stderr, "rom\n"); return 1; }
    GB_set_pixels_output(&gb, screen);
    GB_set_rgb_encode_callback(&gb, rgb_encode);
    GB_set_vblank_callback(&gb, vblank);
    GB_set_palette(&gb, &GB_PALETTE_GREY);
    GB_set_color_correction_mode(&gb, GB_COLOR_CORRECTION_DISABLED);
    GB_set_execution_callback(&gb, exec_cb);
    GB_set_write_memory_callback(&gb, write_cb);
    GB_set_read_memory_callback(&gb, read_cb);
    gb.turbo = true;
    gb.turbo_dont_skip = true;
    while (vblanks < 2000) {
        GB_run(&gb);
        if (done_at >= 0 && vblanks >= done_at + 4) break;
    }
    FILE *f = fopen(argv[4], "wb");
    for (int i = 0; i < 160 * 144; i++) {
        uint8_t p[3] = {screen[i] >> 16, screen[i] >> 8, screen[i]};
        fwrite(p, 1, 3, f);
    }
    fclose(f);
    return done_at < 0 ? 2 : 0;
}
