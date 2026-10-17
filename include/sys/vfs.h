#ifndef _RELIBC_SYS_VFS_H
#define _RELIBC_SYS_VFS_H

#include <sys/types.h>
#include <sys/statvfs.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Linux's statfs over statvfs: the type of the file system is unknown (0). */
struct statfs {
    long f_type;
    long f_bsize;
    fsblkcnt_t f_blocks;
    fsblkcnt_t f_bfree;
    fsblkcnt_t f_bavail;
    fsfilcnt_t f_files;
    fsfilcnt_t f_ffree;
    long f_namelen;
    long f_frsize;
    long f_flags;
};

static inline int statfs(const char *path, struct statfs *buf)
{
    struct statvfs v;
    if (statvfs(path, &v) != 0)
        return -1;
    buf->f_type = 0;
    buf->f_bsize = v.f_bsize;
    buf->f_blocks = v.f_blocks;
    buf->f_bfree = v.f_bfree;
    buf->f_bavail = v.f_bavail;
    buf->f_files = v.f_files;
    buf->f_ffree = v.f_ffree;
    buf->f_namelen = v.f_namemax;
    buf->f_frsize = v.f_frsize;
    buf->f_flags = v.f_flag;
    return 0;
}

#ifdef __cplusplus
}
#endif

#endif
