// Minimal libgba-compatible type header, written for building mgba-emu/suite with a
// stock arm-none-eabi-gcc (no devkitARM/libgba on this machine). Only the surface the
// suite uses is provided; names and semantics follow devkitPro's libgba.
#ifndef GBA_TYPES_H
#define GBA_TYPES_H

#include <stdint.h>

typedef uint8_t  u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef int8_t   s8;
typedef int16_t  s16;
typedef int32_t  s32;

typedef volatile u8  vu8;
typedef volatile u16 vu16;
typedef volatile u32 vu32;
typedef volatile s8  vs8;
typedef volatile s16 vs16;
typedef volatile s32 vs32;

#ifndef __cplusplus
typedef enum { false, true } bool;
#endif

#define NULL ((void*)0)

// Section placement attributes (libgba names). IWRAM_CODE functions live in IWRAM so
// the cycle-counting tests run from fast RAM, exactly as under devkitARM.
#define IWRAM_CODE __attribute__((section(".iwram"), long_call))
#define EWRAM_CODE __attribute__((section(".ewram"), long_call))
#define IWRAM_DATA __attribute__((section(".iwram")))
#define EWRAM_DATA __attribute__((section(".ewram")))
#define IWRAM_BSS  __attribute__((section(".iwram")))
#define EWRAM_BSS  __attribute__((section(".ewram")))

#endif
