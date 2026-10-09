#ifndef CONTINUUM_SMB_MIN_H
#define CONTINUUM_SMB_MIN_H

#include <stddef.h>

/* A directory entry or a share. name is a C string, never longer than 255 bytes. */
typedef struct {
    char name[256];
    int is_dir;
    unsigned long long size;
} SmbEntry;

/* 0 on success. On failure, -1 and a sentence in err (always NUL-terminated). */
int smb_list_shares(const char *host, const char *user, const char *password,
                    SmbEntry *out, int cap, int *count, char *err, size_t err_len);
int smb_list_dir(const char *host, const char *share, const char *path,
                 const char *user, const char *password,
                 SmbEntry *out, int cap, int *count, char *err, size_t err_len);
int smb_download(const char *host, const char *share, const char *path,
                 const char *local_path, const char *user, const char *password,
                 char *err, size_t err_len);

#endif
