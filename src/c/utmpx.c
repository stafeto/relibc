/* utmpx.h: the user accounting database is empty and cannot be written, as
 * on a system without logins to record. */

struct utmpx;
int *__errno_location(void);

#define EPERM 1

void setutxent(void) {}

void endutxent(void) {}

struct utmpx *getutxent(void) { return 0; }

struct utmpx *getutxid(const struct utmpx *id) {
    (void)id;
    return 0;
}

struct utmpx *getutxline(const struct utmpx *line) {
    (void)line;
    return 0;
}

struct utmpx *pututxline(const struct utmpx *entry) {
    (void)entry;
    *__errno_location() = EPERM;
    return 0;
}
