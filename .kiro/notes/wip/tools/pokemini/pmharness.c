// Headless PokeMini (the Pokémon Mini emulator cMini replaces) for comparisons with cMini,
// started the way PokeMini's libretro core starts it (power-on state, then the soft reset that
// runs the start-up program from its second vector). Built by build.sh beside it (PokeMini stays
// in scratch). Usage:
//   pmharness <bios.min|-> <rom.min> trace <instructions> [keys_script]
//   pmharness <bios.min|-> <rom.min> frames <frames> [keys_script]
// ("-" uses PokeMini's own free start-up program.)
// trace: after every instruction (everything but the sound brought up to date after each,
//   PokeMini's finest, waiting for the PRC or not), one line:
//   n V:PC BA I:HL XI:X YI:Y SP F cycles status
// frames: frame by frame as the libretro core runs them (8-cycle catch-ups, the PRC's stall,
//   the sound sampled at 44100 Hz through the piezo filter), one line per frame: frame number,
//   hashes of RAM ($1000-$1FFF), the I/O registers, the LCD's own memory, the analog panel's
//   shades and the frame's sound, its sample count, then the 96x64 pixels as 768 hex bytes.
// start: instruction by instruction until the game's entry ($2102) is reached (at most the
//   given count), then the trace line, the I/O registers as 256 hex bytes, and hashes of RAM and
//   of the LCD's own memory: the state the start-up program hands the game.
// keys_script: lines "frame mask", the mask's bits as $2052 reads them (1 = held, unlike the
//   register) plus 0x100 for the shake; applied before that frame in the libretro core's order.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "PokeMini.h"
#include "Hardware.h"
#include "CommandLine.h"

static uint64_t fnv(const uint8_t* p, int n, uint64_t h) {
	for (int i = 0; i < n; ++i) { h ^= p[i]; h *= 0x100000001b3ULL; }
	return h;
}

static int keyframe[4096], keymask[4096], nkeys;
static int held;

// The libretro core's order: power, up, down, left, right, B, shake, C, then A.
static void apply_keys(int frame) {
	for (int k = 0; k < nkeys; ++k) if (keyframe[k] == frame) held = keymask[k];
	static const int order[8] = { 0x80, 0x08, 0x10, 0x20, 0x40, 0x02, 0x04, 0x01 };
	static const int key[8] = { MINX_KEY_POWER, MINX_KEY_UP, MINX_KEY_DOWN, MINX_KEY_LEFT, MINX_KEY_RIGHT, MINX_KEY_B, MINX_KEY_C, MINX_KEY_A };
	for (int i = 0; i < 6; ++i) PokeMini_KeypadEvent(key[i], (held & order[i]) != 0);
	PokeMini_KeypadEvent(MINX_KEY_SHOCK, (held & 0x100) != 0);
	for (int i = 6; i < 8; ++i) PokeMini_KeypadEvent(key[i], (held & order[i]) != 0);
}

int main(int argc, char** argv) {
	if (argc < 5) { fprintf(stderr, "usage\n"); return 2; }
	CommandLineInit();
	CommandLine.synccycles = 8;
	CommandLine.updatertc = 0;
	if (!PokeMini_Create(0, 0)) { fprintf(stderr, "create failed\n"); return 1; }
	if (strcmp(argv[1], "-") != 0) {
		if (!PokeMini_LoadBIOSFile(argv[1])) { fprintf(stderr, "bios failed\n"); return 1; }
		CommandLine.forcefreebios = 0;
	}
	if (!PokeMini_LoadMINFile(argv[2])) { fprintf(stderr, "rom failed\n"); return 1; }
	MinxIO_FormatEEPROM();
	PokeMini_Reset(0);
	MinxAudio_ChangeFilter(1);
	if (argc > 5) {
		FILE* f = fopen(argv[5], "r");
		while (f && nkeys < 4096 && fscanf(f, "%d %x", &keyframe[nkeys], &keymask[nkeys]) == 2) ++nkeys;
		if (f) fclose(f);
	}
	long n = atol(argv[4]);
	if (!strcmp(argv[3], "start")) {
		for (long i = 0; i < n; ++i) {
			int c = MinxCPU_Exec();
			PokeHWCycles = c;
			MinxTimers_Sync();
			MinxPRC_Sync();
			if (MinxCPU.PC.W.L == 0x2102) {
				printf("%02X:%04X %04X %02X:%04X %02X:%04X %02X:%04X %04X %02X %02X\n", MinxCPU.PC.B.I, MinxCPU.PC.W.L,
					MinxCPU.BA.W.L, MinxCPU.HL.B.I, MinxCPU.HL.W.L, MinxCPU.X.B.I, MinxCPU.X.W.L, MinxCPU.Y.B.I,
					MinxCPU.Y.W.L, MinxCPU.SP.W.L, MinxCPU.F, MinxCPU.N.B.H);
				for (int r = 0; r < 256; ++r) printf("%02X", PM_IO[r]);
				printf("\n%016llx %016llx\n", (unsigned long long) fnv(PM_RAM, 0x1000, 0xcbf29ce484222325ULL),
					(unsigned long long) fnv(LCDData, 256 * 9, 0xcbf29ce484222325ULL));
				return 0;
			}
		}
		printf("never started\n");
		return 0;
	}
	if (!strcmp(argv[3], "trace")) {
		for (long i = 0; i < n; ++i) {
			int c = MinxCPU_Exec();
			PokeHWCycles = c;
			MinxTimers_Sync();
			MinxPRC_Sync();
			printf("%ld %02X:%04X %04X %02X:%04X %02X:%04X %02X:%04X %04X %02X %d %d\n", i,
				MinxCPU.PC.B.I, MinxCPU.PC.W.L, MinxCPU.BA.W.L, MinxCPU.HL.B.I, MinxCPU.HL.W.L,
				MinxCPU.X.B.I, MinxCPU.X.W.L, MinxCPU.Y.B.I, MinxCPU.Y.W.L, MinxCPU.SP.W.L,
				MinxCPU.F, c, MinxCPU.Status);
		}
		return 0;
	}
	for (long fr = 0; fr < n; ++fr) {
		apply_keys((int) fr);
		uint64_t hs = 0xcbf29ce484222325ULL;
		int samples = 0;
		// PokeMini_EmulateFrame, with the sound's catch-up (the libretro core's engine).
		for (;;) {
			PokeHWCycles = 0;
			while (PokeHWCycles < CommandLine.synccycles) {
				if (StallCPU) PokeHWCycles += StallCycles;
				else PokeHWCycles += MinxCPU_Exec();
			}
			MinxTimers_Sync();
			MinxPRC_Sync();
			MinxAudio.AudioCCnt += MINX_AUDIOINC * PokeHWCycles;
			if (MinxAudio.AudioCCnt >= 0x01000000) {
				MinxAudio.AudioCCnt -= 0x01000000;
				int16_t s = MinxAudio_PiezoFilter(MinxAudio_AudioProcessDirectPWM());
				uint8_t b[2] = { (uint8_t) s, (uint8_t) (s >> 8) };
				hs = fnv(b, 2, hs);
				++samples;
			}
			if (MinxPRC.PRCCnt == 0x01000000) break;
		}
		uint64_t h = fnv(PM_RAM, 0x1000, 0xcbf29ce484222325ULL);
		uint64_t hio = fnv(PM_IO, 0x100, 0xcbf29ce484222325ULL);
		uint64_t hl = fnv(LCDData, 256 * 9, 0xcbf29ce484222325ULL);
		uint64_t ha = fnv(LCDPixelsA, 96 * 64, 0xcbf29ce484222325ULL);
		printf("%ld %016llx %016llx %016llx %016llx %016llx %d ", fr, (unsigned long long) h,
			(unsigned long long) hio, (unsigned long long) hl, (unsigned long long) ha, (unsigned long long) hs, samples);
		for (int i = 0; i < 96 * 64; i += 8) {
			int b = 0;
			for (int j = 0; j < 8; ++j) b |= (LCDPixelsD[i + j] ? 1 : 0) << j;
			printf("%02X", b);
		}
		printf("\n");
	}
	return 0;
}
