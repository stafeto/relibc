#include <crypt.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

/* Every failure of crypt() is a null pointer with errno set to EINVAL, never
   a string that could equal a short password field such as "*", "!" or "x". */

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
  if (errno != EINVAL) {
    printf("%s: errno is %d, expected EINVAL\n", what, errno);
    return 1;
  }
  return 0;
}

int main(void) {
  static char key[1025];
  char hash[256], locked[260];
  crypt_data data;
  const char *out;
  int result = 0;
  size_t i, j;

  for (i = 0; i < sizeof(lengths) / sizeof(lengths[0]); ++i) {
    memset(key, 'k', sizeof(key) - 1);
    key[lengths[i]] = '\0';
    for (j = 0; j < sizeof(salts) / sizeof(salts[0]); ++j) {
      char what[64];
      snprintf(what, sizeof(what), "len %zu salt \"%s\"", lengths[i], salts[j]);
      result |= expect_null(what, key, salts[j]);
    }
  }

  /* Settings that start with '$' and holds a byte outside ASCII: no abort. */
  result |= expect_null("salt $6$ e-acute", "pw", "$6$\xc3\xa9");
  result |= expect_null("salt $5$ e-acute", "pw", "$5$ab\xc3\xa9");
  result |= expect_null("salt $6$ rounds e-acute", "pw", "$6$rounds=\xc3\xa9$x");
  result |= expect_null("salt $1$ e-acute at the end", "pw", "$1$aaaaaaa\xc3\xa9");
  result |= expect_null("salt $1$ e-acute inside", "pw", "$1$a\xc3\xa9$");
  result |= expect_null("salt $7$ e-acute", "pw", "$7$CCCCC\xc3\xa9" "aaaaaaaaa");
  /* Other refused settings. */
  result |= expect_null("setting not UTF-8", "pw", "\xff\xfe");
  result |= expect_null("salt $9$", "pw", "$9$abc");
  result |= expect_null("salt $6$ rounds=abc", "pw", "$6$rounds=abc$x");
  result |= expect_null("salt $6$ rounds=+5000", "pw", "$6$rounds=+5000$x");
  result |= expect_null("salt $6$ colon", "pw", "$6$a:b");
  result |= expect_null("salt $6$ rounds=10000000", "pw", "$6$rounds=10000000$x");

  /* The lower bound of the rounds is applied and written out. */
  out = crypt("pw", "$6$rounds=10$x");
  if (out == NULL || strncmp(out, "$6$rounds=1000$x$", 17) != 0) {
    printf("rounds=10: got \"%.30s\"\n", out ? out : "(null)");
    result = 1;
  }

  /* A locked hash is no password field to equal. */
  out = crypt("pw", "$6$saltstring");
  if (out == NULL) {
    printf("hash: NULL\n");
    return 1;
  }
  snprintf(hash, sizeof(hash), "%s", out);
  snprintf(locked, sizeof(locked), "!%s", hash);
  out = crypt("pw", locked);
  if (out != NULL && strcmp(out, locked) == 0) {
    printf("locked hash equals its field\n");
    result = 1;
  }

  /* A failure leaves the buffer of the earlier result alone. */
  memset(&data, 0, sizeof(data));
  out = crypt_r("pw", "$6$saltstring", &data);
  if (out == NULL || strcmp(out, hash) != 0) {
    printf("crypt_r: bad first result\n");
    return 1;
  }
  memset(key, 'k', 300);
  key[300] = '\0';
  if (crypt_r(key, "$6$saltstring", &data) != NULL) {
    printf("crypt_r: a long key gave a string\n");
    result = 1;
  }
  if (strcmp(data.buff, hash) != 0) {
    printf("crypt_r: the buffer changed after a failure\n");
    result = 1;
  }

  return result;
}
