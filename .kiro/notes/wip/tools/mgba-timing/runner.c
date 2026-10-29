// mGBA (the Advance emulator cBoy replaces, at the commit the app ships) run headless on test
// ROMs that end in `B .` with r12 = 0 when they pass (jsmolka's and Alyosha's), for comparing
// cBoy's results with mGBA's. Usage: runner <rom.gba>... ; prints "name r12" (or "name none").
// Built by build.sh beside it; mGBA stays in scratch.
#include <stdio.h>
#include <stdlib.h>
#include <fcntl.h>
#include <mgba/flags.h>
#include <mgba/core/core.h>
#include <mgba/gba/core.h>
#include <mgba/internal/arm/arm.h>
#include <mgba-util/vfs.h>
#include <mgba/core/log.h>

static void quiet(struct mLogger* l, int category, enum mLogLevel level, const char* format, va_list args) {
	(void) l; (void) category; (void) level; (void) format; (void) args;
}

int main(int argc, char** argv) {
	static struct mLogger logger = { .log = quiet };
	mLogSetDefaultLogger(&logger);
	int debug = getenv("RUNNER_DEBUG") != NULL;
	for (int k = 1; k < argc; ++k) {
		struct mCore* core = GBACoreCreate();
		core->init(core);
		mCoreInitConfig(core, NULL);
		static mColor buf[240 * 160];
		core->setVideoBuffer(core, buf, 240);
		struct VFile* vf = VFileOpen(argv[k], O_RDONLY);
		if (!vf || !core->loadROM(core, vf)) { printf("%s load-failed\n", argv[k]); continue; }
		core->reset(core);
		struct ARMCore* cpu = core->cpu;
		int done = 0;
		for (int f = 0; f < 600 && !done; ++f) {
			core->runFrame(core);
			// Between frames mGBA's PC is one instruction past the one it runs (`B .` loops there).
			int thumb = cpu->executionMode == MODE_THUMB;
			uint32_t pc = cpu->gprs[ARM_PC];
			int idle = thumb ? (core->rawRead16(core, pc - 2, -1) == 0xE7FE || core->rawRead16(core, pc - 4, -1) == 0xE7FE)
				: (core->rawRead32(core, pc - 4, -1) == 0xEAFFFFFE || core->rawRead32(core, pc - 8, -1) == 0xEAFFFFFE);
			if (idle) {
				printf("%s %u\n", argv[k], cpu->gprs[12]);
				done = 1;
			}
		}
		if (!done) {
			int thumb = cpu->executionMode == MODE_THUMB;
			uint32_t pc = cpu->gprs[ARM_PC];
			if (debug) printf("%s none (pc %08X thumb %d words %08X %08X %08X r12 %u)\n", argv[k], pc, thumb, core->rawRead32(core, pc - 8, -1), core->rawRead32(core, pc - 4, -1), core->rawRead32(core, pc, -1), cpu->gprs[12]);
			else printf("%s none\n", argv[k]);
		}
		core->deinit(core);
	}
	return 0;
}
