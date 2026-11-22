#include <stdarg.h>
#include <stdio.h>

void continuum_log_line(unsigned level, const char *line);

/* retro_log_printf_t */
void continuum_log_cb(unsigned level, const char *fmt, ...) {
    char buf[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    continuum_log_line(level, buf);
}
