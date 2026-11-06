#include <crypt.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>

/* Every failure of crypt() is a null pointer with errno set, never a string
   that could equal a short password field such as "*", "!" or "x". */

static const char *salts[] = {
  "$5$saltstring", "$6$saltstring", "*", "!", "x", "",
};

static const size_t lengths[] = { 257, 300, 512, 1024 };

static int expect_null(const char *what, const char *key, const char *salt) {
  errno = 0;
  const char *out = crypt(key, salt);
  if (out != NULL) {
    printf("%s: expected NULL, got \"%.20s\"\n", what, out);
    return 1;
  }
  if (errno == 0) {
    printf("%s: errno not set\n", what);
    return 1;
  }
  return 0;
}

int main(void) {
  static char key[1025];
  int result = 0;
  size_t i, j;

  memset(key, 'k', sizeof(key) - 1);

  for (i = 0; i < sizeof(lengths) / sizeof(lengths[0]); ++i) {
    key[lengths[i]] = '\0';
    for (j = 0; j < sizeof(salts) / sizeof(salts[0]); ++j) {
      char what[64];
      snprintf(what, sizeof(what), "len %zu salt \"%s\"", lengths[i], salts[j]);
      result |= expect_null(what, key, salts[j]);
    }
    key[lengths[i]] = 'k';
  }

  /* Salts with a byte outside ASCII must give NULL and leave the process alive. */
  result |= expect_null("salt $6$ e-acute", "pw", "$6$\xc3\xa9");
  result |= expect_null("salt $5$ e-acute", "pw", "$5$ab\xc3\xa9");
  result |= expect_null("salt $6$ rounds e-acute", "pw", "$6$rounds=\xc3\xa9$x");
  result |= expect_null("salt $6$ rounds=abc", "pw", "$6$rounds=abc$x");
  result |= expect_null("salt $6$ colon", "pw", "$6$a:b");
  result |= expect_null("salt $6$ rounds=10000000", "pw", "$6$rounds=10000000$x");

  return result;
}
