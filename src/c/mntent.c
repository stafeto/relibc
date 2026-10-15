/* mntent.h: the mount table is a text file (/etc/mtab on stafeto), a line
 * for each mounted file system: file system, directory, type, options,
 * frequency and pass, separated by blanks. settimeofday sets the real-time
 * clock through clock_settime. */

typedef unsigned long size_t;
typedef struct FILE FILE;
struct mntent {
    char *mnt_fsname;
    char *mnt_dir;
    char *mnt_type;
    char *mnt_opts;
    int mnt_freq;
    int mnt_passno;
};
struct timeval {
    long tv_sec;
    long tv_usec;
};
struct timespec {
    long tv_sec;
    long tv_nsec;
};

FILE *fopen(const char *path, const char *mode);
int fclose(FILE *f);
char *fgets(char *s, int n, FILE *f);
int atoi(const char *s);
char *strstr(const char *a, const char *b);
size_t strlen(const char *s);
int strncmp(const char *a, const char *b, size_t n);
int clock_settime(int clock, const struct timespec *ts);
int clock_gettime(int clock, struct timespec *ts);

FILE *setmntent(const char *file, const char *mode) {
    return fopen(file, mode);
}

int endmntent(FILE *stream) {
    if (stream)
        fclose(stream);
    return 1;
}

char *hasmntopt(const struct mntent *mnt, const char *opt) {
    size_t n = strlen(opt);
    char *p = mnt->mnt_opts;
    while (p && (p = strstr(p, opt)) != 0) {
        if ((p == mnt->mnt_opts || p[-1] == ',') && (p[n] == '\0' || p[n] == ',' || p[n] == '='))
            return p;
        p++;
    }
    return 0;
}

struct mntent *getmntent(FILE *stream) {
    static char line[512];
    static struct mntent entry;
    for (;;) {
        char *f[6] = {0, 0, 0, 0, 0, 0};
        char *p;
        int i;
        if (!fgets(line, sizeof line, stream))
            return 0;
        p = line;
        while (*p == ' ' || *p == '\t')
            p++;
        if (*p == '#' || *p == '\n' || *p == '\0')
            continue;
        for (i = 0; i < 6 && *p; i++) {
            f[i] = p;
            while (*p && *p != ' ' && *p != '\t' && *p != '\n')
                p++;
            if (*p)
                *p++ = '\0';
            while (*p == ' ' || *p == '\t')
                p++;
        }
        if (i < 4)
            continue;
        entry.mnt_fsname = f[0];
        entry.mnt_dir = f[1];
        entry.mnt_type = f[2];
        entry.mnt_opts = f[3];
        entry.mnt_freq = f[4] ? atoi(f[4]) : 0;
        entry.mnt_passno = f[5] ? atoi(f[5]) : 0;
        return &entry;
    }
}

int settimeofday(const struct timeval *tv, const void *tz) {
    struct timespec ts;
    (void)tz;
    if (!tv)
        return 0;
    ts.tv_sec = tv->tv_sec;
    ts.tv_nsec = tv->tv_usec * 1000L;
    return clock_settime(0, &ts);
}

/* wait3: waitpid for any child; no resource usage is accounted, so the
 * usage record is zeros. */
int waitpid(int pid, int *status, int options);
void *memset(void *d, int c, size_t n);

int wait3(int *status, int options, void *rusage) {
    if (rusage)
        memset(rusage, 0, 144);
    return waitpid(-1, status, options);
}

/* sysinfo: the time since boot from the monotonic clock; the other figures
 * are zeros (the system keeps no load averages and counts no memory here). */
struct sysinfo {
    long uptime;
    unsigned long loads[3];
    unsigned long totalram, freeram, sharedram, bufferram, totalswap, freeswap;
    unsigned short procs, pad;
    unsigned long totalhigh, freehigh;
    unsigned int mem_unit;
    char _f[20 - 2 * sizeof(long) - sizeof(int)];
};

int sysinfo(struct sysinfo *info) {
    struct timespec ts;
    memset(info, 0, sizeof *info);
    if (clock_gettime(1, &ts) != 0)
        return -1;
    info->uptime = ts.tv_sec;
    info->mem_unit = 1;
    return 0;
}
