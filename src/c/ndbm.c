/* ndbm.h: a database of records kept in memory and written whole to
 * "<name>.pag" after each change. */

typedef unsigned long size_t;
typedef unsigned int mode_t;
typedef long ssize_t;
void *malloc(size_t n);
void *realloc(void *p, size_t n);
void free(void *p);
void *memcpy(void *d, const void *s, size_t n);
int memcmp(const void *a, const void *b, size_t n);
size_t strlen(const char *s);
int open(const char *path, int flags, ...);
int close(int fd);
ssize_t read(int fd, void *buf, size_t n);
ssize_t write(int fd, const void *buf, size_t n);
int *__errno_location(void);

typedef struct { void *dptr; size_t dsize; } datum;

struct record {
    void *key;
    size_t klen;
    void *data;
    size_t dlen;
};

typedef struct DBM {
    struct record *records;
    size_t count;
    size_t cursor;
    int error;
    int writable;
    char *path;
} DBM;

#define EACCES 13
#define ENOMEM 12
#define O_ACCMODE 3
#define O_RDONLY 0
#define O_TRUNC 0x200
#define O_WRONLY 1

static int save(DBM *db) {
    int fd = open(db->path, O_WRONLY | O_TRUNC, 0);
    if (fd < 0) return -1;
    for (size_t i = 0; i < db->count; i++) {
        struct record *r = &db->records[i];
        size_t lens[2] = { r->klen, r->dlen };
        const void *parts[3] = { lens, r->key, r->data };
        size_t sizes[3] = { sizeof lens, r->klen, r->dlen };
        for (int p = 0; p < 3; p++) {
            const char *b = parts[p];
            size_t left = sizes[p];
            while (left) {
                ssize_t n = write(fd, b, left);
                if (n <= 0) { close(fd); return -1; }
                b += n;
                left -= (size_t)n;
            }
        }
    }
    return close(fd);
}

static int load(DBM *db, int fd) {
    for (;;) {
        size_t lens[2];
        char *b = (char *)lens;
        size_t left = sizeof lens;
        while (left) {
            ssize_t n = read(fd, b, left);
            if (n < 0) return -1;
            if (n == 0) return left == sizeof lens ? 0 : -1;
            b += n;
            left -= (size_t)n;
        }
        struct record r;
        r.klen = lens[0];
        r.dlen = lens[1];
        r.key = malloc(r.klen ? r.klen : 1);
        r.data = malloc(r.dlen ? r.dlen : 1);
        if (!r.key || !r.data) { free(r.key); free(r.data); return -1; }
        for (int p = 0; p < 2; p++) {
            b = p ? r.data : r.key;
            left = p ? r.dlen : r.klen;
            while (left) {
                ssize_t n = read(fd, b, left);
                if (n <= 0) { free(r.key); free(r.data); return -1; }
                b += n;
                left -= (size_t)n;
            }
        }
        struct record *grown = realloc(db->records, (db->count + 1) * sizeof r);
        if (!grown) { free(r.key); free(r.data); return -1; }
        db->records = grown;
        db->records[db->count++] = r;
    }
}

static long find(DBM *db, datum key) {
    for (size_t i = 0; i < db->count; i++) {
        struct record *r = &db->records[i];
        if (r->klen == key.dsize && memcmp(r->key, key.dptr, key.dsize) == 0) return (long)i;
    }
    return -1;
}

DBM *dbm_open(const char *file, int open_flags, mode_t file_mode) {
    size_t n = strlen(file);
    DBM *db = malloc(sizeof *db);
    char *path = malloc(n + 5);
    if (!db || !path) { free(db); free(path); *__errno_location() = ENOMEM; return 0; }
    memcpy(path, file, n);
    memcpy(path + n, ".pag", 5);
    db->records = 0;
    db->count = 0;
    db->cursor = 0;
    db->error = 0;
    db->writable = (open_flags & O_ACCMODE) != O_RDONLY;
    db->path = path;
    int fd = open(path, open_flags & ~O_TRUNC, file_mode);
    if (fd < 0) { free(db); free(path); return 0; }
    if ((open_flags & O_TRUNC) == 0 && load(db, fd) < 0) {
        int saved = *__errno_location();
        close(fd);
        free(db->records);
        free(db);
        free(path);
        *__errno_location() = saved ? saved : 5;
        return 0;
    }
    close(fd);
    if ((open_flags & O_TRUNC) && db->writable && save(db) < 0) {
        int saved = *__errno_location();
        free(db); free(path);
        *__errno_location() = saved;
        return 0;
    }
    return db;
}

void dbm_close(DBM *db) {
    if (!db) return;
    for (size_t i = 0; i < db->count; i++) {
        free(db->records[i].key);
        free(db->records[i].data);
    }
    free(db->records);
    free(db->path);
    free(db);
}

datum dbm_fetch(DBM *db, datum key) {
    datum none = { 0, 0 };
    long at = find(db, key);
    if (at < 0) return none;
    datum found = { db->records[at].data, db->records[at].dlen };
    return found;
}

int dbm_store(DBM *db, datum key, datum content, int mode) {
    if (!db->writable) { *__errno_location() = EACCES; db->error = 1; return -1; }
    long at = find(db, key);
    if (at >= 0) {
        if (mode == 0) return 1;
        void *data = malloc(content.dsize ? content.dsize : 1);
        if (!data) { *__errno_location() = ENOMEM; db->error = 1; return -1; }
        memcpy(data, content.dptr, content.dsize);
        free(db->records[at].data);
        db->records[at].data = data;
        db->records[at].dlen = content.dsize;
    } else {
        struct record r;
        r.key = malloc(key.dsize ? key.dsize : 1);
        r.data = malloc(content.dsize ? content.dsize : 1);
        struct record *grown = realloc(db->records, (db->count + 1) * sizeof r);
        if (!r.key || !r.data || !grown) {
            free(r.key); free(r.data);
            if (grown) db->records = grown;
            *__errno_location() = ENOMEM;
            db->error = 1;
            return -1;
        }
        memcpy(r.key, key.dptr, key.dsize);
        memcpy(r.data, content.dptr, content.dsize);
        r.klen = key.dsize;
        r.dlen = content.dsize;
        db->records = grown;
        db->records[db->count++] = r;
    }
    if (save(db) < 0) { db->error = 1; return -1; }
    return 0;
}

int dbm_delete(DBM *db, datum key) {
    if (!db->writable) { *__errno_location() = EACCES; db->error = 1; return -1; }
    long at = find(db, key);
    if (at < 0) return 1;
    free(db->records[at].key);
    free(db->records[at].data);
    for (size_t i = (size_t)at + 1; i < db->count; i++) db->records[i - 1] = db->records[i];
    db->count--;
    if (save(db) < 0) { db->error = 1; return -1; }
    return 0;
}

datum dbm_firstkey(DBM *db) {
    db->cursor = 0;
    return dbm_nextkey(db);
}

datum dbm_nextkey(DBM *db) {
    datum none = { 0, 0 };
    if (db->cursor >= db->count) return none;
    struct record *r = &db->records[db->cursor++];
    datum k = { r->key, r->klen };
    return k;
}

int dbm_error(DBM *db) { return db->error; }

int dbm_clearerr(DBM *db) {
    db->error = 0;
    return 0;
}
