/* Libretro GET_LOG_INTERFACE callback.
 *
 * The frontend must hand the core a printf-style varargs function pointer. Rust cannot
 * express that ABI on stable, so the trampoline lives here. On Apple the line goes to
 * os_log (Console.app / Xcode); elsewhere it goes to stderr. The last line is kept for the
 * host status UI.
 */
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#if defined(__APPLE__)
#include <os/log.h>
#endif

enum { CONTINUUM_LAST_LOG_CAP = 1024 };

static char g_last_log[CONTINUUM_LAST_LOG_CAP];
static int g_have_last_log = 0;

const char *continuum_last_core_log_line(void)
{
    return g_have_last_log ? g_last_log : "";
}

void continuum_retro_log_printf(int level, const char *fmt, ...)
{
    char buf[2048];
    va_list ap;
    size_t n;

    if (fmt == NULL) {
        return;
    }

    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);

    n = strlen(buf);
    while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r')) {
        buf[--n] = '\0';
    }

    strncpy(g_last_log, buf, CONTINUUM_LAST_LOG_CAP - 1);
    g_last_log[CONTINUUM_LAST_LOG_CAP - 1] = '\0';
    g_have_last_log = 1;

#if defined(__APPLE__)
    {
        os_log_t log = os_log_create("app.continuum", "libretro");
        os_log_type_t type = OS_LOG_TYPE_INFO;
        if (level <= 0) {
            type = OS_LOG_TYPE_DEBUG;
        } else if (level == 2) {
            type = OS_LOG_TYPE_DEFAULT;
        } else if (level >= 3) {
            type = OS_LOG_TYPE_ERROR;
        }
        os_log_with_type(log, type, "%{public}s", buf);
    }
#else
    {
        const char *tag = "INFO";
        if (level <= 0) {
            tag = "DEBUG";
        } else if (level == 2) {
            tag = "WARN";
        } else if (level >= 3) {
            tag = "ERROR";
        }
        fprintf(stderr, "[libretro:%s] %s\n", tag, buf);
        fflush(stderr);
    }
#endif
    (void)level;
}

/* data is struct retro_log_callback { retro_log_printf_t log; }. */
void continuum_fill_retro_log_callback(void *data)
{
    if (data == NULL) {
        return;
    }
    *(void **)data = (void *)&continuum_retro_log_printf;
}
