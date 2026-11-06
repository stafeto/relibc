#include <grp.h>
#include <pwd.h>
#include <shadow.h>
#include <stdio.h>
#include <string.h>

/* A name that is a proper prefix of an account or group name finds nothing:
   lookups compare the whole name. A lookup may find an entry only when the
   entry carries exactly the name asked for. */

static int failures;

static void fail(const char *what, const char *name) {
  printf("%s(\"%s\") found a different name\n", what, name);
  ++failures;
}

static void check(const char *name) {
  char buf[1024];
  struct passwd pw, *pwp;
  struct group gr, *grp;
  struct spwd sp, *spp;
  struct passwd *p = getpwnam(name);
  struct group *g = getgrnam(name);
  struct spwd *s = getspnam(name);

  if (p != NULL && strcmp(p->pw_name, name) != 0) fail("getpwnam", name);
  if (g != NULL && strcmp(g->gr_name, name) != 0) fail("getgrnam", name);
  if (s != NULL && strcmp(s->sp_namp, name) != 0) fail("getspnam", name);

  pwp = NULL;
  if (getpwnam_r(name, &pw, buf, sizeof(buf), &pwp) == 0 && pwp != NULL &&
      strcmp(pw.pw_name, name) != 0)
    fail("getpwnam_r", name);
  grp = NULL;
  if (getgrnam_r(name, &gr, buf, sizeof(buf), &grp) == 0 && grp != NULL &&
      strcmp(gr.gr_name, name) != 0)
    fail("getgrnam_r", name);
  spp = NULL;
  if (getspnam_r(name, &sp, buf, sizeof(buf), &spp) == 0 && spp != NULL &&
      strcmp(sp.sp_namp, name) != 0)
    fail("getspnam_r", name);
}

static void check_prefixes(const char *name) {
  char prefix[256];
  size_t len = strlen(name), i;

  if (len >= sizeof(prefix)) return;
  for (i = 0; i < len; ++i) {
    memcpy(prefix, name, i);
    prefix[i] = '\0';
    check(prefix);
  }
}

int main(void) {
  struct passwd *pw;
  struct group *gr;
  char names[64][64];
  size_t count = 0, i;

  /* The empty name is a prefix of every name. */
  check("");

  setpwent();
  while (count < 64 && (pw = getpwent()) != NULL) {
    if (strlen(pw->pw_name) < sizeof(names[0]))
      strcpy(names[count++], pw->pw_name);
  }
  endpwent();
  for (i = 0; i < count; ++i) check_prefixes(names[i]);

  count = 0;
  setgrent();
  while (count < 64 && (gr = getgrent()) != NULL) {
    if (strlen(gr->gr_name) < sizeof(names[0]))
      strcpy(names[count++], gr->gr_name);
  }
  endgrent();
  for (i = 0; i < count; ++i) check_prefixes(names[i]);

  printf("%d failures\n", failures);
  return failures != 0;
}
