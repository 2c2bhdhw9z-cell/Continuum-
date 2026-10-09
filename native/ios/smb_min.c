#include "smb_min.h"

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* libsmb2.h first. libsmb2-raw.h uses types that header defines. */
#include <smb2/libsmb2.h>
#include <smb2/libsmb2-raw.h>
#include <smb2/libsmb2-share-enum.h>

static void smb_fail(char *err, size_t err_len, struct smb2_context *smb2, const char *fallback) {
    const char *text = smb2 ? smb2_get_error(smb2) : NULL;
    if (!text || !text[0]) {
        text = fallback;
    }
    if (err && err_len) {
        snprintf(err, err_len, "%s", text);
    }
}

static struct smb2_context *smb_open(const char *host, const char *share, const char *user,
                                     const char *password, char *err, size_t err_len) {
    struct smb2_context *smb2 = smb2_init_context();
    if (!smb2) {
        smb_fail(err, err_len, NULL, "could not start SMB");
        return NULL;
    }
    smb2_set_user(smb2, user && user[0] ? user : "guest");
    smb2_set_password(smb2, password ? password : "");
    smb2_set_timeout(smb2, 20);
    if (smb2_connect_share(smb2, host, share, user && user[0] ? user : "guest") != 0) {
        smb_fail(err, err_len, smb2, "could not connect");
        smb2_destroy_context(smb2);
        return NULL;
    }
    return smb2;
}

static const char *smb_rel(const char *path) {
    if (!path) {
        return "";
    }
    while (*path == '/') {
        path++;
    }
    return path;
}

int smb_list_shares(const char *host, const char *user, const char *password, SmbEntry *out,
                    int cap, int *count, char *err, size_t err_len) {
    struct smb2_context *smb2;
    struct smb2_share_enum_reply *reply;
    uint32_t i;
    int n = 0;
    if (count) {
        *count = 0;
    }
    if (!host || !host[0] || !out || cap <= 0) {
        smb_fail(err, err_len, NULL, "SMB was given no server");
        return -1;
    }
    smb2 = smb_open(host, "IPC$", user, password, err, err_len);
    if (!smb2) {
        return -1;
    }
    reply = smb2_share_enum_sync(smb2, SMB2_SHARE_INFO_1);
    if (!reply) {
        smb_fail(err, err_len, smb2, "could not list shares");
        smb2_disconnect_share(smb2);
        smb2_destroy_context(smb2);
        return -1;
    }
    for (i = 0; i < reply->entries_read && n < cap; i++) {
        struct smb2_share_info_1 *info = &reply->share_info.info_1[i];
        uint32_t kind = info->type & 3u;
        if (!info->netname || !info->netname[0]) {
            continue;
        }
        if (kind != SMB2_SHARE_TYPE_DISKTREE) {
            continue;
        }
        if (info->type & SMB2_SHARE_TYPE_HIDDEN) {
            continue;
        }
        memset(&out[n], 0, sizeof out[n]);
        snprintf(out[n].name, sizeof out[n].name, "%s", info->netname);
        out[n].is_dir = 1;
        n++;
    }
    smb2_free_data(smb2, reply);
    smb2_disconnect_share(smb2);
    smb2_destroy_context(smb2);
    if (count) {
        *count = n;
    }
    return 0;
}

int smb_list_dir(const char *host, const char *share, const char *path, const char *user,
                 const char *password, SmbEntry *out, int cap, int *count, char *err,
                 size_t err_len) {
    struct smb2_context *smb2;
    struct smb2dir *dir;
    struct smb2dirent *ent;
    int n = 0;
    if (count) {
        *count = 0;
    }
    if (!host || !share || !share[0] || !out || cap <= 0) {
        smb_fail(err, err_len, NULL, "SMB was given no share");
        return -1;
    }
    smb2 = smb_open(host, share, user, password, err, err_len);
    if (!smb2) {
        return -1;
    }
    dir = smb2_opendir(smb2, smb_rel(path));
    if (!dir) {
        smb_fail(err, err_len, smb2, "could not open the folder");
        smb2_disconnect_share(smb2);
        smb2_destroy_context(smb2);
        return -1;
    }
    while ((ent = smb2_readdir(smb2, dir)) != NULL && n < cap) {
        if (!ent->name || !ent->name[0] || strcmp(ent->name, ".") == 0 ||
            strcmp(ent->name, "..") == 0) {
            continue;
        }
        memset(&out[n], 0, sizeof out[n]);
        snprintf(out[n].name, sizeof out[n].name, "%s", ent->name);
        out[n].is_dir = ent->st.smb2_type == SMB2_TYPE_DIRECTORY;
        out[n].size = ent->st.smb2_size;
        n++;
    }
    smb2_closedir(smb2, dir);
    smb2_disconnect_share(smb2);
    smb2_destroy_context(smb2);
    if (count) {
        *count = n;
    }
    return 0;
}

int smb_download(const char *host, const char *share, const char *path, const char *local_path,
                 const char *user, const char *password, char *err, size_t err_len) {
    struct smb2_context *smb2;
    struct smb2fh *fh;
    FILE *out;
    uint8_t buf[64 * 1024];
    uint64_t offset = 0;
    if (!host || !share || !local_path) {
        smb_fail(err, err_len, NULL, "SMB was given no file");
        return -1;
    }
    smb2 = smb_open(host, share, user, password, err, err_len);
    if (!smb2) {
        return -1;
    }
    fh = smb2_open(smb2, smb_rel(path), O_RDONLY);
    if (!fh) {
        smb_fail(err, err_len, smb2, "could not open the file");
        smb2_disconnect_share(smb2);
        smb2_destroy_context(smb2);
        return -1;
    }
    out = fopen(local_path, "wb");
    if (!out) {
        smb_fail(err, err_len, NULL, "could not write the download");
        smb2_close(smb2, fh);
        smb2_disconnect_share(smb2);
        smb2_destroy_context(smb2);
        return -1;
    }
    for (;;) {
        int got = smb2_pread(smb2, fh, buf, (uint32_t)sizeof buf, offset);
        if (got == 0) {
            break;
        }
        if (got < 0) {
            smb_fail(err, err_len, smb2, "the download stopped");
            fclose(out);
            smb2_close(smb2, fh);
            smb2_disconnect_share(smb2);
            smb2_destroy_context(smb2);
            return -1;
        }
        if (fwrite(buf, 1, (size_t)got, out) != (size_t)got) {
            smb_fail(err, err_len, NULL, "could not write the download");
            fclose(out);
            smb2_close(smb2, fh);
            smb2_disconnect_share(smb2);
            smb2_destroy_context(smb2);
            return -1;
        }
        offset += (uint64_t)got;
    }
    fclose(out);
    smb2_close(smb2, fh);
    smb2_disconnect_share(smb2);
    smb2_destroy_context(smb2);
    return 0;
}
