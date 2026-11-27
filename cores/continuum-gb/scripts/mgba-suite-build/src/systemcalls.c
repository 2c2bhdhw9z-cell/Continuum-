// BIOS-call wrappers. Each one puts its arguments in the registers the BIOS expects
// (r0-r3, in order) and issues the SWI. The "swi n" mnemonic assembles in both ARM
// and Thumb mode; the BIOS reads the comment byte.
#include <gba_systemcalls.h>

void Halt(void) {
    __asm__ volatile("swi 2" ::: "memory");
}

void Stop(void) {
    __asm__ volatile("swi 3" ::: "memory");
}

void IntrWait(u32 flagClear, u32 irq) {
    register u32 r0 __asm__("r0") = flagClear;
    register u32 r1 __asm__("r1") = irq;
    __asm__ volatile("swi 4" : "+r"(r0) : "r"(r1) : "r2", "r3", "memory");
}

void VBlankIntrWait(void) {
    __asm__ volatile("swi 5" ::: "r0", "r1", "r2", "r3", "memory");
}

int Div(int num, int den) {
    register int r0 __asm__("r0") = num;
    register int r1 __asm__("r1") = den;
    __asm__ volatile("swi 6" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r0;
}

int DivArm(int den, int num) {
    register int r0 __asm__("r0") = den;
    register int r1 __asm__("r1") = num;
    __asm__ volatile("swi 7" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r0;
}

int Mod(int num, int den) {
    register int r0 __asm__("r0") = num;
    register int r1 __asm__("r1") = den;
    __asm__ volatile("swi 6" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r1;
}

int ModArm(int den, int num) {
    register int r0 __asm__("r0") = den;
    register int r1 __asm__("r1") = num;
    __asm__ volatile("swi 7" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r1;
}

int Sqrt(u32 num) {
    register u32 r0 __asm__("r0") = num;
    __asm__ volatile("swi 8" : "+r"(r0) : : "r1", "r2", "r3", "memory");
    return (int)r0;
}

int ArcTan(s16 x, s16 y) {
    register s16 r0 __asm__("r0") = x;
    register s16 r1 __asm__("r1") = y;
    __asm__ volatile("swi 9" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r0;
}

int ArcTan2(s16 x, s16 y) {
    register s16 r0 __asm__("r0") = x;
    register s16 r1 __asm__("r1") = y;
    __asm__ volatile("swi 10" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
    return r0;
}

void CpuSet(const void* source, void* dest, u32 mode) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    register u32 r2 __asm__("r2") = mode;
    __asm__ volatile("swi 11" : "+r"(r0), "+r"(r1), "+r"(r2) : : "r3", "memory");
}

void CpuFastSet(const void* source, void* dest, u32 mode) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    register u32 r2 __asm__("r2") = mode;
    __asm__ volatile("swi 12" : "+r"(r0), "+r"(r1), "+r"(r2) : : "r3", "memory");
}

void RegisterRamReset(u32 flags) {
    register u32 r0 __asm__("r0") = flags;
    __asm__ volatile("swi 1" : "+r"(r0) : : "r1", "r2", "r3", "memory");
}

void SoftReset(void) {
    __asm__ volatile("swi 0" ::: "r0", "r1", "r2", "r3", "memory");
}

void BgAffineSet(const void* source, void* dest, int num) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    register int r2 __asm__("r2") = num;
    __asm__ volatile("swi 14" : "+r"(r0), "+r"(r1), "+r"(r2) : : "r3", "memory");
}

void ObjAffineSet(const void* source, void* dest, int num, int offset) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    register int r2 __asm__("r2") = num;
    register int r3 __asm__("r3") = offset;
    __asm__ volatile("swi 15" : "+r"(r0), "+r"(r1), "+r"(r2), "+r"(r3) : : "memory");
}

void BitUnPack(const void* source, void* dest, const void* unpackInfo) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    register const void* r2 __asm__("r2") = unpackInfo;
    __asm__ volatile("swi 16" : "+r"(r0), "+r"(r1), "+r"(r2) : : "r3", "memory");
}

void LZ77UnCompVram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 17" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void LZ77UnCompWram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 18" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void HuffUnComp(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 19" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void RLUnCompVram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 20" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void RLUnCompWram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 21" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void Diff8bitUnFilterVram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 22" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void Diff8bitUnFilterWram(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 23" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void Diff16bitUnFilter(const void* source, void* dest) {
    register const void* r0 __asm__("r0") = source;
    register void* r1 __asm__("r1") = dest;
    __asm__ volatile("swi 24" : "+r"(r0), "+r"(r1) : : "r2", "r3", "memory");
}

void SoundBias(u32 bias) {
    register u32 r0 __asm__("r0") = bias;
    __asm__ volatile("swi 25" : "+r"(r0) : : "r1", "r2", "r3", "memory");
}

void SoundDriverInit(void* work) {
    register void* r0 __asm__("r0") = work;
    __asm__ volatile("swi 26" : "+r"(r0) : : "r1", "r2", "r3", "memory");
}

void SoundDriverMode(u32 mode) {
    register u32 r0 __asm__("r0") = mode;
    __asm__ volatile("swi 27" : "+r"(r0) : : "r1", "r2", "r3", "memory");
}

void SoundDriverMain(void) {
    __asm__ volatile("swi 28" ::: "r0", "r1", "r2", "r3", "memory");
}

void SoundDriverVSync(void) {
    __asm__ volatile("swi 29" ::: "r0", "r1", "r2", "r3", "memory");
}

void SoundChannelClear(void) {
    __asm__ volatile("swi 30" ::: "r0", "r1", "r2", "r3", "memory");
}

// MidiKey2Freq hands the BIOS a WaveData pointer in r0 and gets a frequency
// back in the same register, so r0 is a pointer on the way in and a number on
// the way out: it is declared as the word it is and the pointer is cast into it.
u32 MidiKey2Freq(void* waveData, u8 key, u8 fineAdjust) {
    register u32 r0 __asm__("r0") = (u32) waveData;
    register u32 r1 __asm__("r1") = key;
    register u32 r2 __asm__("r2") = fineAdjust;
    __asm__ volatile("swi 31" : "+r"(r0), "+r"(r1), "+r"(r2) : : "r3", "memory");
    return r0;
}

void SoundDriverVSyncOff(void) {
    __asm__ volatile("swi 40" ::: "r0", "r1", "r2", "r3", "memory");
}

void SoundDriverVSyncOn(void) {
    __asm__ volatile("swi 41" ::: "r0", "r1", "r2", "r3", "memory");
}

u32 SoundGetJumpList(void) {
    register u32 r0 __asm__("r0");
    __asm__ volatile("swi 42" : "=r"(r0) : : "r1", "r2", "r3", "memory");
    return r0;
}
