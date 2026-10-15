#ifndef _RELIBC_MNTENT_H
#define _RELIBC_MNTENT_H

#include <stdio.h>

#define MNTTYPE_IGNORE "ignore"
#define MNTOPT_RO "ro"
#define MNTOPT_RW "rw"

#ifdef __cplusplus
extern "C" {
#endif

struct mntent {
    char *mnt_fsname;
    char *mnt_dir;
    char *mnt_type;
    char *mnt_opts;
    int mnt_freq;
    int mnt_passno;
};

FILE *setmntent(const char *file, const char *mode);
struct mntent *getmntent(FILE *stream);
int endmntent(FILE *stream);
char *hasmntopt(const struct mntent *mnt, const char *opt);

#ifdef __cplusplus
}
#endif

#endif
