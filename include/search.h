#ifndef _RELIBC_SEARCH_H
#define _RELIBC_SEARCH_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct entry {
    char *key;
    void *data;
} ENTRY;

typedef enum { FIND, ENTER } ACTION;

typedef enum { preorder, postorder, endorder, leaf } VISIT;

typedef void posix_tnode;

int hcreate(size_t nel);
void hdestroy(void);
ENTRY *hsearch(ENTRY item, ACTION action);

void insque(void *element, void *pred);
void remque(void *element);

void *lfind(const void *key, const void *base, size_t *nelp, size_t width,
            int (*compar)(const void *, const void *));
void *lsearch(const void *key, void *base, size_t *nelp, size_t width,
              int (*compar)(const void *, const void *));

posix_tnode *tdelete(const void *restrict key, posix_tnode **restrict rootp,
                     int (*compar)(const void *, const void *));
posix_tnode *tfind(const void *key, posix_tnode *const *rootp,
                   int (*compar)(const void *, const void *));
posix_tnode *tsearch(const void *key, posix_tnode **rootp,
                     int (*compar)(const void *, const void *));
void twalk(const posix_tnode *root,
           void (*action)(const posix_tnode *, VISIT, int));
void twalk_r(const posix_tnode *root,
             void (*action)(const posix_tnode *, VISIT, void *), void *closure);

#ifdef __cplusplus
}
#endif

#endif
