/* The identity of a statically linked program for __cxa_atexit and
 * __cxa_thread_atexit_impl (the Itanium C++ ABI's DSO handle). A shared
 * object carries its own; this one belongs to the program. Hidden, so that
 * no other object sees it. */
__attribute__((visibility("hidden"))) void *__dso_handle = 0;
