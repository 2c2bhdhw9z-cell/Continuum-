// Minimal libgba-compatible compression header.
#ifndef GBA_COMPRESSION_H
#define GBA_COMPRESSION_H

#include <gba_base.h>

void LZ77UnCompVram(const void* source, void* dest);
void LZ77UnCompWram(const void* source, void* dest);
void HuffUnComp(const void* source, void* dest);
void RLUnCompVram(const void* source, void* dest);
void RLUnCompWram(const void* source, void* dest);
void Diff8bitUnFilterVram(const void* source, void* dest);
void Diff8bitUnFilterWram(const void* source, void* dest);
void Diff16bitUnFilter(const void* source, void* dest);
void BitUnPack(const void* source, void* dest, const void* unpackInfo);

#endif
