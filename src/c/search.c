/* The tables and trees of search.h. The tree is a balanced binary tree of
 * heights (an AVL tree), so that sorted keys do not make a list of it. */

typedef unsigned long size_t;
void *malloc(size_t n);
void *calloc(size_t n, size_t m);
void free(void *p);
int strcmp(const char *a, const char *b);
void *memcpy(void *d, const void *s, size_t n);

/* --- queues --- */

struct qelem {
    struct qelem *next;
    struct qelem *prev;
};

void insque(void *element, void *pred) {
    struct qelem *e = element, *p = pred;
    if (!p) {
        e->next = e->prev = 0;
        return;
    }
    e->next = p->next;
    e->prev = p;
    p->next = e;
    if (e->next) e->next->prev = e;
}

void remque(void *element) {
    struct qelem *e = element;
    if (e->next) e->next->prev = e->prev;
    if (e->prev) e->prev->next = e->next;
}

/* --- linear tables --- */

void *lfind(const void *key, const void *base, size_t *nelp, size_t width,
            int (*compar)(const void *, const void *)) {
    char *p = (char *)base;
    size_t n = *nelp;
    for (size_t i = 0; i < n; i++, p += width)
        if (compar(key, p) == 0) return p;
    return 0;
}

void *lsearch(const void *key, void *base, size_t *nelp, size_t width,
              int (*compar)(const void *, const void *)) {
    void *found = lfind(key, base, nelp, width, compar);
    if (found) return found;
    char *slot = (char *)base + *nelp * width;
    memcpy(slot, key, width);
    (*nelp)++;
    return slot;
}

/* --- hash table --- */

typedef struct { char *key; void *data; } ENTRY;
typedef enum { FIND, ENTER } ACTION;

static ENTRY *table;
static size_t table_size;
static size_t table_used;

static size_t hash_of(const char *s) {
    size_t h = 5381;
    while (*s) h = h * 33 + (unsigned char)*s++;
    return h;
}

int hcreate(size_t nel) {
    if (table) return 0;
    /* A table a third larger than asked, so that probes stay short. */
    size_t size = 8;
    while (size < nel + nel / 3 + 1) size <<= 1;
    table = calloc(size, sizeof(ENTRY));
    if (!table) return 0;
    table_size = size;
    table_used = 0;
    return 1;
}

void hdestroy(void) {
    free(table);
    table = 0;
    table_size = table_used = 0;
}

ENTRY *hsearch(ENTRY item, ACTION action) {
    if (!table) return 0;
    size_t at = hash_of(item.key) & (table_size - 1);
    for (size_t probes = 0; probes < table_size; probes++) {
        ENTRY *slot = &table[(at + probes) & (table_size - 1)];
        if (!slot->key) {
            if (action == FIND || table_used >= table_size - 1) return 0;
            *slot = item;
            table_used++;
            return slot;
        }
        if (strcmp(slot->key, item.key) == 0) return slot;
    }
    return 0;
}

/* --- trees --- */

typedef enum { preorder, postorder, endorder, leaf } VISIT;

struct node {
    const void *key;
    void *a[2];
    int h;
};

#define MAXH (sizeof(void *) * 8 * 3 / 2)

static int height(struct node *n) { return n ? n->h : 0; }

static int rot(void **p, struct node *x, int dir) {
    struct node *y = x->a[dir];
    struct node *z = y->a[!dir];
    int hx = height(x->a[!dir]);
    int hz = height(z);
    if (hz > height(y->a[!dir])) {
        x->a[dir] = z->a[!dir];
        y->a[!dir] = z->a[dir];
        z->a[!dir] = x;
        z->a[dir] = y;
        x->h = hz;
        y->h = hz;
        z->h = hz + 1;
    } else {
        x->a[dir] = z;
        y->a[!dir] = x;
        x->h = hz + 1;
        y->h = hz + 2;
        z = y;
    }
    *p = z;
    return z->h - hx;
}

/* Balances *p; returns 0 when the height did not change. */
static int balance(void **p) {
    struct node *n = *p;
    int h0 = height(n->a[0]);
    int h1 = height(n->a[1]);
    if (h0 - h1 + 1u < 3u) {
        int old = n->h;
        n->h = h0 < h1 ? h1 + 1 : h0 + 1;
        return n->h - old;
    }
    return rot(p, n, h0 < h1);
}

void *tsearch(const void *key, void **rootp, int (*cmp)(const void *, const void *)) {
    if (!rootp) return 0;
    void **a[MAXH];
    struct node *n = *rootp;
    int i = 0;
    a[i++] = rootp;
    for (;;) {
        if (!n) break;
        int c = cmp(key, n->key);
        if (!c) return n;
        a[i++] = &n->a[c > 0];
        n = n->a[c > 0];
    }
    struct node *r = malloc(sizeof *r);
    if (!r) return 0;
    r->key = key;
    r->a[0] = r->a[1] = 0;
    r->h = 1;
    *a[--i] = r;
    while (i && balance(a[--i]));
    return r;
}

void *tfind(const void *key, void *const *rootp, int (*cmp)(const void *, const void *)) {
    if (!rootp) return 0;
    struct node *n = *rootp;
    for (;;) {
        if (!n) return 0;
        int c = cmp(key, n->key);
        if (!c) return n;
        n = n->a[c > 0];
    }
}

void *tdelete(const void *key, void **rootp, int (*cmp)(const void *, const void *)) {
    if (!rootp) return 0;
    void **a[MAXH + 1];
    struct node *n = *rootp;
    struct node *parent;
    struct node *child;
    int i = 0;
    /* *a[0] is an arbitrary non-null pointer, returned when the root goes. */
    a[i++] = rootp;
    a[i++] = rootp;
    for (;;) {
        if (!n) return 0;
        int c = cmp(key, n->key);
        if (!c) break;
        a[i++] = &n->a[c > 0];
        n = n->a[c > 0];
    }
    parent = *a[i - 2];
    if (n->a[0]) {
        struct node *deleted = n;
        a[i++] = &n->a[0];
        n = n->a[0];
        while (n->a[1]) {
            a[i++] = &n->a[1];
            n = n->a[1];
        }
        deleted->key = n->key;
        child = n->a[0];
    } else {
        child = n->a[1];
    }
    free(n);
    *a[--i] = child;
    while (--i && balance(a[i]));
    return parent;
}

static void walk(const struct node *r, void (*action)(const void *, VISIT, int), int d) {
    if (!r) return;
    if (r->h == 1) {
        action(r, leaf, d);
    } else {
        action(r, preorder, d);
        walk(r->a[0], action, d + 1);
        action(r, postorder, d);
        walk(r->a[1], action, d + 1);
        action(r, endorder, d);
    }
}

void twalk(const void *root, void (*action)(const void *, VISIT, int)) {
    walk(root, action, 0);
}

static void walk_r(const struct node *r, void (*action)(const void *, VISIT, void *), void *closure) {
    if (!r) return;
    if (r->h == 1) {
        action(r, leaf, closure);
    } else {
        action(r, preorder, closure);
        walk_r(r->a[0], action, closure);
        action(r, postorder, closure);
        walk_r(r->a[1], action, closure);
        action(r, endorder, closure);
    }
}

void twalk_r(const void *root, void (*action)(const void *, VISIT, void *), void *closure) {
    walk_r(root, action, closure);
}
