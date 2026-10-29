// Differential harness: mGBA's own GBA cheat parser and cheat program, against a fake memory, so
// cBoy's (cores/continuum-gb/src/gba/cheats.rs) can be compared with it line for line.
// Built by build.sh beside it; mGBA's source and the binary stay in scratch. Reads cases from stdin, one per line: the code's lines joined
// with '|'. Prints, per case: the hook, the ROM patches as bytes, and every byte the program
// wrote (address:value), sorted, after running it twice.
#include <mgba/core/core.h>
#include <mgba/core/cheats.h>
#include <mgba/core/log.h>
#include <mgba/internal/gba/cheats.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// mGBA's logging and breakpoints: not needed here.
void mLog(int category, enum mLogLevel level, const char* format, ...) { (void) category; (void) level; (void) format; }
int mLogGenerateCategory(const char* name, const char* id) { (void) name; (void) id; return 0; }
void GBASetBreakpoint(void* gba, struct mCPUComponent* component, uint32_t address, enum ExecutionMode mode, uint32_t* opcode) {
	(void) gba; (void) component; (void) address; (void) mode; (void) opcode;
}
void GBAClearBreakpoint(void* gba, uint32_t address, enum ExecutionMode mode, uint32_t opcode) {
	(void) gba; (void) address; (void) mode; (void) opcode;
}
const struct mCoreMemoryBlock* mCoreGetMemoryBlockInfo(const struct mCore* core, uint32_t address) { (void) core; (void) address; return NULL; }

#define MAXW 1048576
#define HSIZE (1 << 22)
static uint32_t waddr[MAXW];
static uint8_t wval[MAXW];
static int nw;
static int slot_of[HSIZE]; // index + 1 into waddr, 0 for empty
static uint32_t hkey[HSIZE];
static uint32_t wslot[MAXW];

static uint8_t pattern(uint32_t a) {
	uint32_t h = a * 0x9E3779B1u;
	return h >> 24;
}
static uint32_t hpos(uint32_t a) {
	uint32_t h = (a * 0x85EBCA6Bu) ^ (a >> 13);
	return h & (HSIZE - 1);
}
static int find(uint32_t a) {
	for (uint32_t p = hpos(a);; p = (p + 1) & (HSIZE - 1)) {
		if (!slot_of[p]) return -1;
		if (hkey[p] == a) return slot_of[p] - 1;
	}
}
static uint8_t rd8(uint32_t a) {
	int i = find(a);
	return i >= 0 ? wval[i] : pattern(a);
}
static void wr8(uint32_t a, uint8_t v) {
	int i = find(a);
	if (i >= 0) {
		wval[i] = v;
		return;
	}
	if (nw >= MAXW) return;
	waddr[nw] = a;
	wval[nw] = v;
	uint32_t p = hpos(a);
	while (slot_of[p]) p = (p + 1) & (HSIZE - 1);
	slot_of[p] = nw + 1;
	hkey[p] = a;
	wslot[nw] = p;
	++nw;
}
static void clear_mem(void) {
	for (int k = 0; k < nw; ++k) slot_of[wslot[k]] = 0;
	nw = 0;
}
static uint32_t busRead8(struct mCore* c, uint32_t a) { (void) c; return rd8(a); }
static uint32_t busRead16(struct mCore* c, uint32_t a) { (void) c; return rd8(a) | rd8(a + 1) << 8; }
static uint32_t busRead32(struct mCore* c, uint32_t a) { (void) c; return rd8(a) | rd8(a + 1) << 8 | rd8(a + 2) << 16 | (uint32_t) rd8(a + 3) << 24; }
static void busWrite8(struct mCore* c, uint32_t a, uint8_t v) { (void) c; wr8(a, v); }
static void busWrite16(struct mCore* c, uint32_t a, uint16_t v) { (void) c; wr8(a, v); wr8(a + 1, v >> 8); }
static void busWrite32(struct mCore* c, uint32_t a, uint32_t v) { (void) c; wr8(a, v); wr8(a + 1, v >> 8); wr8(a + 2, v >> 16); wr8(a + 3, v >> 24); }
static uint32_t rawRead8(struct mCore* c, uint32_t a, int s) { (void) c; (void) a; (void) s; return 0; }
static uint32_t rawRead16(struct mCore* c, uint32_t a, int s) { (void) c; (void) a; (void) s; return 0; }
static uint32_t rawRead32(struct mCore* c, uint32_t a, int s) { (void) c; (void) a; (void) s; return 0; }
static void rawWrite8(struct mCore* c, uint32_t a, int s, uint8_t v) { (void) c; (void) a; (void) s; (void) v; }
static void rawWrite16(struct mCore* c, uint32_t a, int s, uint16_t v) { (void) c; (void) a; (void) s; (void) v; }
static void rawWrite32(struct mCore* c, uint32_t a, int s, uint32_t v) { (void) c; (void) a; (void) s; (void) v; }

static int cmp(const void* x, const void* y) {
	uint32_t a = waddr[*(const int*) x], b = waddr[*(const int*) y];
	return a < b ? -1 : a > b;
}

int main(void) {
	static struct mCore core;
	memset(&core, 0, sizeof(core));
	core.busRead8 = busRead8;
	core.busRead16 = busRead16;
	core.busRead32 = busRead32;
	core.busWrite8 = busWrite8;
	core.busWrite16 = busWrite16;
	core.busWrite32 = busWrite32;
	core.rawRead8 = rawRead8;
	core.rawRead16 = rawRead16;
	core.rawRead32 = rawRead32;
	core.rawWrite8 = rawWrite8;
	core.rawWrite16 = rawWrite16;
	core.rawWrite32 = rawWrite32;
	static char buf[1 << 16];
	static int order[MAXW];
	while (fgets(buf, sizeof(buf), stdin)) {
		buf[strcspn(buf, "\r\n")] = 0;
		struct mCheatDevice* device = GBACheatDeviceCreate();
		struct mCheatSet* set = device->createSet(device, NULL);
		char* save = NULL;
		for (char* line = strtok_r(buf, "|", &save); line; line = strtok_r(NULL, "|", &save)) {
			mCheatAddLine(set, line, GBA_CHEAT_AUTODETECT);
		}
		struct GBACheatSet* gset = (struct GBACheatSet*) set;
		if (gset->hook) {
			printf("hook %08X", gset->hook->address);
		} else {
			printf("hook none");
		}
		size_t i;
		printf(" | patches");
		for (i = 0; i < mCheatPatchListSize(&set->romPatches); ++i) {
			struct mCheatPatch* p = mCheatPatchListGetPointer(&set->romPatches, i);
			for (int b = 0; b < p->width; ++b) {
				printf(" %07X:%02X", (p->address & 0x01FFFFFF) + b, (p->value >> (8 * b)) & 0xFF);
			}
		}
		clear_mem();
		device->p = &core;
		set->enabled = true;
		// Twice, so adds and conditions on written values show.
		mCheatRefresh(device, set);
		mCheatRefresh(device, set);
		printf(" | writes");
		for (int k = 0; k < nw; ++k) order[k] = k;
		qsort(order, nw, sizeof(int), cmp);
		for (int k = 0; k < nw; ++k) {
			printf(" %08X:%02X", waddr[order[k]], wval[order[k]]);
		}
		printf("\n");
		fflush(stdout);
		device->p = NULL;
		mCheatSetDeinit(set);

	}
	return 0;
}
