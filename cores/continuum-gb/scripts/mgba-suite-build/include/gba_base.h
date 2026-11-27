// Minimal libgba-compatible base header.
#ifndef GBA_BASE_H
#define GBA_BASE_H

#include <gba_types.h>

#define REG_BASE 0x04000000

// Cartridge RAM, where the suite writes its results (savprintf). A bare
// address: the suite casts and ORs it (e.g. SRAM | 0x01000000 for mirrors).
#define SRAM 0x0E000000
#define BIT(n) (1 << (n))

// Call BIOS function n (the SWI comment byte). Arguments are passed in r0-r3 by the
// normal calling convention; the BIOS may clobber r0-r3.
#define SystemCall(n) __asm__ volatile("swi\t" #n ::: "r0", "r1", "r2", "r3")

#endif
