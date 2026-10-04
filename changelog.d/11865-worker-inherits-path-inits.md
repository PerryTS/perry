Fixed a Worker failing with `MODULE_NOT_FOUND` on a `require` that works on the
main thread, when the required module is reached only through a function-local
or conditional `require` (turndown's `@mixmark-io/domino`). A worker now
adopts its spawner's module initializers at spawn.
