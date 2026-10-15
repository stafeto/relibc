/* utmpx.h: the user accounting database is the file /var/run/utmpx, records
 * of struct utmpx one after another. A system without logins has none or an
 * empty one: the readers then find nothing. The writer needs the file or
 * the right to make it. */

typedef unsigned long size_t;
typedef long ssize_t;
typedef long off_t;
typedef int pid_t;

struct utmpx {
    char ut_user[32];
    char ut_id[4];
    char ut_line[32];
    pid_t ut_pid;
    short ut_type;
    struct {
        long tv_sec;
        long tv_usec;
    } ut_tv;
};

int *__errno_location(void);
int open(const char *path, int flags, ...);
int close(int fd);
ssize_t read(int fd, void *buf, size_t n);
ssize_t write(int fd, const void *buf, size_t n);
off_t lseek(int fd, off_t offset, int whence);
int strncmp(const char *a, const char *b, size_t n);
void *memcpy(void *d, const void *s, size_t n);

#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0100
#define O_CLOEXEC 02000000
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define ENOENT 2
#define EPERM 1

#define BOOT_TIME 2
#define NEW_TIME 3
#define OLD_TIME 4
#define INIT_PROCESS 5
#define LOGIN_PROCESS 6
#define USER_PROCESS 7
#define DEAD_PROCESS 8

#define DATABASE "/var/run/utmpx"

static char database_path[256] = DATABASE;

static int database = -1;
static int writable;
static struct utmpx entry;
/* The offset after the record last read into `entry`, or -1. */
static off_t last_end = -1;

/* Open the database for reading (or for writing too when `write` is set).
 * A file that is not there is an empty database for the readers. */
static int open_database(int want_write) {
    int saved = *__errno_location();
    if (database >= 0 && (writable || !want_write))
        return 0;
    if (database >= 0) {
        close(database);
        database = -1;
    }
    if (want_write) {
        database = open(database_path, O_RDWR | O_CREAT | O_CLOEXEC, 0644);
        writable = database >= 0;
    } else {
        database = open(database_path, O_RDWR | O_CLOEXEC);
        writable = database >= 0;
        if (database < 0)
            database = open(database_path, 0 | O_CLOEXEC);
    }
    /* No file is an empty database: no error for the readers. */
    if (!want_write)
        *__errno_location() = saved;
    return database < 0 ? -1 : 0;
}

/* The next record into `entry`; 1 for one, 0 at the end or with no file. */
static int next_record(void) {
    size_t have = 0;
    if (open_database(0) < 0)
        return 0;
    while (have < sizeof entry) {
        ssize_t n = read(database, (char *)&entry + have, sizeof entry - have);
        if (n <= 0)
            return 0;
        have += (size_t)n;
    }
    last_end = lseek(database, 0, SEEK_CUR);
    return 1;
}

/* The file of the database, for the programs that keep their own (a
 * common extension): 0, or -1 for a name that does not fit. */
void endutxent(void);

int utmpxname(const char *file) {
    size_t n = 0;
    while (file[n])
        n++;
    if (n == 0 || n >= sizeof database_path)
        return -1;
    endutxent();
    memcpy(database_path, file, n + 1);
    return 0;
}

void setutxent(void) {
    last_end = -1;
    if (database >= 0)
        lseek(database, 0, SEEK_SET);
}

void endutxent(void) {
    if (database >= 0)
        close(database);
    database = -1;
    last_end = -1;
}

struct utmpx *getutxent(void) {
    return next_record() ? &entry : 0;
}

/* Whether the record `have` is the one `want` asks for as getutxid does. */
static int id_matches(const struct utmpx *have, const struct utmpx *want) {
    switch (want->ut_type) {
    case BOOT_TIME:
    case OLD_TIME:
    case NEW_TIME:
        return have->ut_type == want->ut_type;
    case INIT_PROCESS:
    case LOGIN_PROCESS:
    case USER_PROCESS:
    case DEAD_PROCESS:
        return have->ut_type >= INIT_PROCESS && have->ut_type <= DEAD_PROCESS &&
               strncmp(have->ut_id, want->ut_id, sizeof have->ut_id) == 0;
    default:
        return 0;
    }
}

struct utmpx *getutxid(const struct utmpx *id) {
    while (next_record()) {
        if (id_matches(&entry, id))
            return &entry;
    }
    return 0;
}

struct utmpx *getutxline(const struct utmpx *line) {
    while (next_record()) {
        if ((entry.ut_type == LOGIN_PROCESS || entry.ut_type == USER_PROCESS) &&
            strncmp(entry.ut_line, line->ut_line, sizeof entry.ut_line) == 0)
            return &entry;
    }
    return 0;
}

/* Writes `put` over the record that getutxid finds for it, searching
 * forward from the current position, or at the end of the file when there
 * is none. The record last read counts as found when it matches. */
struct utmpx *pututxline(const struct utmpx *put) {
    struct utmpx copy;
    off_t at;
    memcpy(&copy, put, sizeof copy);
    /* Nobody may change the records of a system without logins: EPERM. */
    if (open_database(1) < 0) {
        *__errno_location() = EPERM;
        return 0;
    }
    at = lseek(database, 0, SEEK_CUR);
    if (at < 0)
        return 0;
    if (last_end >= 0 && at == last_end && id_matches(&entry, &copy)) {
        lseek(database, at - (off_t)sizeof entry, SEEK_SET);
    } else {
        for (;;) {
            struct utmpx here;
            off_t where = lseek(database, 0, SEEK_CUR);
            ssize_t n = read(database, &here, sizeof here);
            if (n != (ssize_t)sizeof here) {
                lseek(database, 0, SEEK_END);
                break;
            }
            if (id_matches(&here, &copy)) {
                lseek(database, where, SEEK_SET);
                break;
            }
        }
    }
    last_end = -1;
    if (write(database, &copy, sizeof copy) != (ssize_t)sizeof copy) {
        *__errno_location() = EPERM;
        return 0;
    }
    memcpy(&entry, &copy, sizeof entry);
    return &entry;
}
