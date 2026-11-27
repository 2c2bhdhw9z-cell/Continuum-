// Minimal libgba-compatible BIOS-call header. The wrappers are implemented in
// systemcalls.c with explicit register constraints so the arguments land in r0-r3
// exactly as the BIOS expects.
#ifndef GBA_SYSTEMCALLS_H
#define GBA_SYSTEMCALLS_H

#include <gba_base.h>

void Halt(void);
void Stop(void);
void IntrWait(u32 flagClear, u32 irq);
void VBlankIntrWait(void);
int Div(int num, int den);
int DivArm(int den, int num);
int Mod(int num, int den);
int ModArm(int den, int num);
int Sqrt(u32 num);
int ArcTan(s16 x, s16 y);
int ArcTan2(s16 x, s16 y);
void CpuSet(const void* source, void* dest, u32 mode);
void CpuFastSet(const void* source, void* dest, u32 mode);
void RegisterRamReset(u32 flags);
void SoftReset(void);
void BgAffineSet(const void* source, void* dest, int num);
void ObjAffineSet(const void* source, void* dest, int num, int offset);
void BitUnPack(const void* source, void* dest, const void* unpackInfo);
void LZ77UnCompVram(const void* source, void* dest);
void LZ77UnCompWram(const void* source, void* dest);
void HuffUnComp(const void* source, void* dest);
void RLUnCompVram(const void* source, void* dest);
void RLUnCompWram(const void* source, void* dest);
void Diff8bitUnFilterVram(const void* source, void* dest);
void Diff8bitUnFilterWram(const void* source, void* dest);
void Diff16bitUnFilter(const void* source, void* dest);
void SoundBias(u32 bias);
void SoundDriverInit(void* work);
void SoundDriverMode(u32 mode);
void SoundDriverMain(void);
void SoundDriverVSync(void);
void SoundChannelClear(void);
u32 MidiKey2Freq(void* waveData, u8 key, u8 fineAdjust);
void SoundDriverVSyncOff(void);
void SoundDriverVSyncOn(void);
u32 SoundGetJumpList(void);

// CpuSet/CpuFastSet mode flags.
#define COPY16 0x00000000
#define COPY32 0x04000000
#define FILL   0x01000000

#endif
