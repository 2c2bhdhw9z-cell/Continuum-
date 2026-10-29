// Headless PokeMini (the Pokémon Mini emulator cMini replaces) for comparisons with cMini.
// Built by build.sh beside it (PokeMini stays in scratch). Usage:
//   pmharness <bios.min|-> <rom.min> trace <instructions> [keys_script]
//   pmharness <bios.min|-> <rom.min> frames <frames> [keys_script]
// trace: after every instruction (hardware brought up to date after each, PokeMini's finest),
//   one line: n pc ba hl x y sp f e(xi/yi as in the registers) cycles status
// frames: after every frame: frame number, a hash of RAM ($1000-$1FFF), the I/O registers and
//   the LCD's own memory, and the 96x64 picture as 768 hex bytes.
// keys_script: lines "frame mask" (bits as the $2052 register reads, 1 = released).
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

static void apply_keys(int frame) {
	for (int k = 0; k < nkeys; ++k) {
		if (keyframe[k] == frame) {
			for (int b = 0; b < 8; ++b) {
				int released = (keymask[k] >> b) & 1;
				PokeMini_KeypadEvent(b, !released);
			}
		}
	}
}

int main(int argc, char** argv) {
	if (argc < 5) { fprintf(stderr, "usage\n"); return 2; }
	CommandLineInit();
	CommandLine.synccycles = 8;
	CommandLine.updatertc = 0;
	if (!PokeMini_Create(0, 0)) { fprintf(stderr, "create failed\n"); return 1; }
	if (strcmp(argv[1], "-") != 0) {
		if (!PokeMini_LoadBIOSFile(argv[1])) { fprintf(stderr, "bios failed\n"); return 1; }
	}
	if (!PokeMini_LoadMINFile(argv[2])) { fprintf(stderr, "rom failed\n"); return 1; }
	PokeMini_Reset(1);
	if (argc > 5) {
		FILE* f = fopen(argv[5], "r");
		while (f && nkeys < 4096 && fscanf(f, "%d %x", &keyframe[nkeys], &keymask[nkeys]) == 2) ++nkeys;
		if (f) fclose(f);
	}
	long n = atol(argv[4]);
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
	} else {
		for (long fr = 0; fr < n; ++fr) {
			apply_keys(fr);
			PokeMini_EmulateFrame();
			uint64_t h = fnv(PM_RAM, 0x1000, 0xcbf29ce484222325ULL);
			uint64_t hio = fnv(PM_IO, 0x100, 0xcbf29ce484222325ULL);
			uint64_t hl = fnv(LCDData, 256 * 9, 0xcbf29ce484222325ULL);
			printf("%ld %016llx %016llx %016llx ", fr, (unsigned long long) h, (unsigned long long) hio, (unsigned long long) hl);
			for (int i = 0; i < 96 * 64; i += 8) {
				int b = 0;
				for (int j = 0; j < 8; ++j) b |= (LCDPixelsD[i + j] ? 1 : 0) << j;
				printf("%02X", b);
			}
			printf("\n");
		}
	}
	return 0;
}
