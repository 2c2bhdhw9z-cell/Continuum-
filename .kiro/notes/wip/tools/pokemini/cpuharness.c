// PokeMini's own MINX processor (the emulator cMini replaces), on its own, over a fake memory:
// one instruction per case, for cMini's processor to be compared with it (SingleStepTests style).
// Reads cases from stdin, one per line, hex:
//   seed BA HL X Y SP PC F E N I XI YI V U SHIFTU b0 b1 b2 b3 b4 b5
// (the six bytes are written at PC's address; every other byte reads as a pattern of the seed).
// Prints: BA HL X Y SP PC F E N I XI YI V U1 SHIFTU cycles status | writes addr:val ... | the
// interrupt check points the instruction passed (F:shift_u, in order)
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "PokeMini.h"

#define MAXW 4096
static uint32_t seed;
static uint32_t waddr[MAXW];
static uint8_t wval[MAXW];
static int nw;
static uint32_t paddr[16];
static uint8_t pval[16];
static int np;

static uint8_t pattern(uint32_t a) {
	uint32_t h = (a ^ seed) * 0x9E3779B1u;
	h ^= h >> 15;
	h *= 0x85EBCA6Bu;
	return h >> 24;
}
static int findw(uint32_t a) {
	for (int i = nw - 1; i >= 0; --i) if (waddr[i] == a) return i;
	return -1;
}
uint8_t MinxCPU_OnRead(int cpu, uint32_t addr) {
	(void) cpu;
	addr &= 0x1FFFFF;
	int i = findw(addr);
	if (i >= 0) return wval[i];
	for (int k = 0; k < np; ++k) if (paddr[k] == addr) return pval[k];
	return pattern(addr);
}
void MinxCPU_OnWrite(int cpu, uint32_t addr, uint8_t data) {
	(void) cpu;
	addr &= 0x1FFFFF;
	if (nw < MAXW) { waddr[nw] = addr; wval[nw] = data; ++nw; }
}
void MinxCPU_OnException(int type, uint32_t opc) { (void) type; (void) opc; }
void MinxCPU_OnSleep(int type) { (void) type; }
static uint8_t chkf[64], chks[64];
static int nchk;
void MinxCPU_OnIRQHandle(uint8_t flag, uint8_t shift_u) { if (nchk < 64) { chkf[nchk] = flag; chks[nchk] = shift_u; ++nchk; } }
void MinxCPU_OnIRQAct(uint8_t intr) { (void) intr; }

int main(void) {
	char line[512];
	while (fgets(line, sizeof(line), stdin)) {
		unsigned v[22];
		if (sscanf(line, "%x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x %x",
			&v[0], &v[1], &v[2], &v[3], &v[4], &v[5], &v[6], &v[7], &v[8], &v[9], &v[10], &v[11],
			&v[12], &v[13], &v[14], &v[15], &v[16], &v[17], &v[18], &v[19], &v[20], &v[21]) != 22) {
			printf("bad\n");
			continue;
		}
		seed = v[0];
		memset(&MinxCPU, 0, sizeof(MinxCPU));
		MinxCPU.BA.W.L = v[1];
		MinxCPU.HL.W.L = v[2];
		MinxCPU.X.W.L = v[3];
		MinxCPU.Y.W.L = v[4];
		MinxCPU.SP.W.L = v[5];
		MinxCPU.PC.W.L = v[6];
		MinxCPU.F = v[7];
		MinxCPU.E = v[8];
		MinxCPU.N.B.H = v[9];
		MinxCPU.HL.B.I = v[10];
		MinxCPU.N.B.I = v[10];
		MinxCPU.X.B.I = v[11];
		MinxCPU.Y.B.I = v[12];
		MinxCPU.PC.B.I = v[13];
		MinxCPU.U1 = v[14];
		MinxCPU.U2 = v[14];
		MinxCPU.Shift_U = v[15];
		MinxCPU.Status = MINX_STATUS_NORMAL;
		uint32_t pc = (v[6] & 0x8000) ? ((v[6] & 0x7FFF) | (v[13] << 15)) : v[6];
		np = 0;
		for (int k = 0; k < 6; ++k) { paddr[np] = (pc + k) & 0x1FFFFF; pval[np] = v[16 + k]; ++np; }
		nw = 0;
		nchk = 0;
		int c = MinxCPU_Exec();
		printf("%04X %04X %04X %04X %04X %04X %02X %02X %02X %02X %02X %02X %02X %02X %d %d %d |",
			MinxCPU.BA.W.L, MinxCPU.HL.W.L, MinxCPU.X.W.L, MinxCPU.Y.W.L, MinxCPU.SP.W.L, MinxCPU.PC.W.L,
			MinxCPU.F, MinxCPU.E, MinxCPU.N.B.H, MinxCPU.HL.B.I, MinxCPU.X.B.I, MinxCPU.Y.B.I,
			MinxCPU.PC.B.I, MinxCPU.U1, MinxCPU.Shift_U, c, MinxCPU.Status);
		for (int k = 0; k < nw; ++k) printf(" %06X:%02X", waddr[k], wval[k]);
		printf(" |");
		for (int k = 0; k < nchk; ++k) printf(" %02X:%d", chkf[k], chks[k]);
		printf("\n");
	}
	return 0;
}
