// newlib support for the suite build: devkitPro's devoptab_list, which
// mgba.c's mgba_console_open writes to. The suite never calls
// mgba_console_open (it uses mgba_printf / savprintf), so this is just a
// link-time anchor. Syscalls themselves come from -specs=nosys.specs.
#include <sys/iosupport.h>

const devoptab_t* devoptab_list[3] = {0, 0, 0};
