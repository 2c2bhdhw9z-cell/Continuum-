// Minimal libgba-compatible SIO header.
#ifndef GBA_SIO_H
#define GBA_SIO_H

#include <gba_base.h>

#define REG_SIODATA32_L (*(vu16*)0x4000120)
#define REG_SIODATA32_H (*(vu16*)0x4000122)
#define REG_SIODATA32   (*(vu32*)0x4000120)
#define REG_SIOCNT      (*(vu16*)0x4000128)
#define REG_SIODATA8    (*(vu16*)0x400012A)
#define REG_RCNT        (*(vu16*)0x4000134)
#define REG_JOYCNT      (*(vu16*)0x4000140)
#define REG_JOY_RECV    (*(vu32*)0x4000150)
#define REG_JOY_TRANS   (*(vu32*)0x4000154)
#define REG_JOYSTAT     (*(vu16*)0x4000158)

#define SIODATA32_L REG_SIODATA32_L
#define SIODATA32_H REG_SIODATA32_H
#define SIODATA32 REG_SIODATA32
#define SIOCNT REG_SIOCNT
#define SIODATA8 REG_SIODATA8
#define SIOMULTI0 (*(vu16*)0x4000120)
#define SIOMULTI1 (*(vu16*)0x4000122)
#define SIOMULTI2 (*(vu16*)0x4000124)
#define SIOMULTI3 (*(vu16*)0x4000126)
#define SIOMLT_SEND (*(vu16*)0x400012A)

#define SIO_8BIT   0x0000
#define SIO_32BIT  0x1000
#define SIO_MULTI  0x0000
#define SIO_UART   0x2000
#define SIO_UART   0x2000
#define SIO_IRQ    0x4000
#define SIO_START  0x8000
#define SIO_CLOCK_INTERNAL 0x0000
#define SIO_CLOCK_EXTERNAL 0x0001

// RCNT mode: general-purpose I/O (the link port's pins as plain GPIO).
#define R_GPIO 0x8000
#define SIO_BAUD_9600  0x0000
#define SIO_BAUD_38400 0x0001
#define SIO_BAUD_57600 0x0002
#define SIO_BAUD_115200 0x0003

#endif
