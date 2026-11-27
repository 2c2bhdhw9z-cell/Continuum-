// Minimal libgba-compatible sound header.
#ifndef GBA_SOUND_H
#define GBA_SOUND_H

#include <gba_base.h>

#define REG_SOUNDCNT_L (*(vu16*)0x4000060)
#define REG_SOUNDCNT_H (*(vu16*)0x4000062)
#define REG_SOUNDCNT_X (*(vu16*)0x4000084)
#define REG_SOUNDBIAS  (*(vu16*)0x4000088)
#define REG_WAVE_RAM   ((vu16*)0x4000090)
#define REG_FIFO_A      (*(vu32*)0x40000A0)
#define REG_FIFO_B      (*(vu32*)0x40000A4)

#endif
