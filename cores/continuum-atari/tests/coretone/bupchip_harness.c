/* Plays music through the original CoreTone, driven by ProSystem's BupChip layer (its
 * bupchip_ProcessAudioCommand is what a write to $8007 does), for tests/coretone.rs to compare
 * with ours sample for sample.
 *
 * Build it against ProSystem's sources (libretro/prosystem-libretro, with PR #71's BupChip):
 *   cc -O2 -fwrapv -I$PS/core -I$PS/libretro-common/include -o bupchip_harness \
 *      bupchip_harness.c $PS/core/BupChip.c $PS/bupboop/coretone/[a-z]*.c -lm
 * Run:
 *   bupchip_harness <dir> <steps> <out>
 * <dir>/files.txt is the CORETONE part of a .cdf (the sample package, the instrument package,
 * then the songs), <dir>/commands.txt has "<step> <byte>" lines in order (the byte is written to
 * $8007 just before that step), and <out> gets every step's 200 stereo samples, 16-bit little
 * endian. -fwrapv makes the C's signed overflow wrap, as every compiler it runs on does anyway.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <boolean.h>
#include "BupChip.h"

/* What BupChip.c needs from ProSystem's Cartridge.c: the next line that isn't empty, and a
 * file's contents from beside the .cdf. */
char *cartridge_GetNextNonemptyLine(const char **stream, size_t *size)
{
   while (*size != 0)
   {
      const char *line = *stream;
      const char *end;
      while (*size > 0 && **stream != '\r' && **stream != '\n')
      {
         (*stream)++;
         (*size)--;
      }
      end = *stream;
      while (*size > 0 && (**stream == '\r' || **stream == '\n'))
      {
         (*stream)++;
         (*size)--;
      }
      if (end != line)
      {
         char *out = malloc(end - line + 1);
         memcpy(out, line, end - line);
         out[end - line] = 0;
         return out;
      }
   }
   return NULL;
}

static uint8_t *slurp(const char *path, size_t *size)
{
   FILE *f = fopen(path, "rb");
   uint8_t *data;
   long n;
   if (!f)
      return NULL;
   fseek(f, 0, SEEK_END);
   n = ftell(f);
   fseek(f, 0, SEEK_SET);
   /* One spare byte, zero, as libretro's filestream_read_file leaves after a file. */
   data = calloc(1, (size_t)n + 1);
   if (fread(data, 1, (size_t)n, f) != (size_t)n)
   {
      free(data);
      data = NULL;
   }
   fclose(f);
   *size = (size_t)n;
   return data;
}

bool cartridge_ReadFile(uint8_t **outData, size_t *outSize, const char *subpath, const char *relativeTo)
{
   char path[4096];
   snprintf(path, sizeof path, "%s/%s", relativeTo, subpath);
   *outData = slurp(path, outSize);
   return *outData != NULL && *outSize > 0;
}

int main(int argc, char **argv)
{
   char path[4096];
   size_t size;
   const char *text;
   uint8_t *files;
   FILE *commands, *out;
   long steps, step, at;
   unsigned byte;
   int more;
   int16_t buffer[CORETONE_BUFFER_LEN];

   if (argc != 4)
   {
      fprintf(stderr, "usage: bupchip_harness <dir> <steps> <out>\n");
      return 2;
   }
   snprintf(path, sizeof path, "%s/files.txt", argv[1]);
   files = slurp(path, &size);
   if (!files)
      return 1;
   text = (const char *)files;
   if (!bupchip_InitFromCDF(&text, &size, argv[1]))
   {
      fprintf(stderr, "the music files didn't load\n");
      return 1;
   }
   snprintf(path, sizeof path, "%s/commands.txt", argv[1]);
   commands = fopen(path, "r");
   out = fopen(argv[3], "wb");
   if (!commands || !out)
      return 1;
   steps = atol(argv[2]);
   more = fscanf(commands, "%ld %u", &at, &byte) == 2;
   for (step = 0; step < steps; step++)
   {
      while (more && at == step)
      {
         bupchip_ProcessAudioCommand((unsigned char)byte);
         more = fscanf(commands, "%ld %u", &at, &byte) == 2;
      }
      ct_update(buffer);
      fwrite(buffer, sizeof buffer[0], CORETONE_BUFFER_LEN, out);
   }
   fclose(out);
   return 0;
}
