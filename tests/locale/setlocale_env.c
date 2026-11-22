#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "test_helpers.h"

static void check(const char *want) {
    char *got = setlocale(LC_ALL, "");
    if (got == NULL || strcmp(got, want) != 0) {
        printf("setlocale(LC_ALL, \"\") returned %s, wanted %s\n", got ? got : "NULL", want);
        exit(EXIT_FAILURE);
    }
}

int main(void) {
    // "" resolves by LC_ALL, then LC_<category>, then LANG, then "C".
    unsetenv("LC_ALL");
    unsetenv("LC_CTYPE");
    unsetenv("LANG");
    check("C");
    setenv("LANG", "C.UTF-8", 1);
    check("C.UTF-8");
    setenv("LC_ALL", "", 1);
    check("C.UTF-8");
    setenv("LC_ALL", "POSIX", 1);
    check("POSIX");
    // A name no file defines fails and leaves the locale as it was.
    setenv("LC_ALL", "no-such-locale", 1);
    if (setlocale(LC_ALL, "") != NULL) {
        puts("an unknown name was accepted");
        exit(EXIT_FAILURE);
    }
    // A UTF-8 name of the environment with no definition takes the data of
    // C.UTF-8 and keeps its name; the same name given by the program fails.
    setenv("LC_ALL", "xx_XX.UTF-8", 1);
    check("xx_XX.UTF-8");
    if (setlocale(LC_ALL, "yy_YY.UTF-8") != NULL) {
        puts("an unknown explicit name was accepted");
        exit(EXIT_FAILURE);
    }
    unsetenv("LC_ALL");
    check("C.UTF-8");
    puts("success!");
}
