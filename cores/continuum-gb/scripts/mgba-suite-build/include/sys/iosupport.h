// Stub for devkitPro's <sys/iosupport.h>, which stock arm-none-eabi newlib does
// not ship. mgba-emu/suite's mgba.c uses it for the devoptab_t stdout hook
// (mgba_console_open) and for struct _reent / ssize_t. Only the first five
// fields of devoptab_t are used here (name, structsize, open_r, close_r,
// write_r), so the rest of the struct is omitted; devoptab_list itself is
// defined in newlib-stubs.c.
#ifndef SYS_IOSUPPORT_H
#define SYS_IOSUPPORT_H

#include <sys/types.h>

struct _reent;

typedef struct {
    const char* name;
    int structsize;
    int (*open_r)(struct _reent* r, void* fileStruct, const char* path,
                  int flags, int mode);
    int (*close_r)(struct _reent* r, void* fd);
    ssize_t (*write_r)(struct _reent* r, void* fd, const char* ptr,
                       size_t len);
    ssize_t (*read_r)(struct _reent* r, void* fd, char* ptr, size_t len);
} devoptab_t;

extern const devoptab_t* devoptab_list[];

#define STD_IN 0
#define STD_OUT 1
#define STD_ERR 2

#endif
