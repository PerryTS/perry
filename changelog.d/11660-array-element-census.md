`PERRY_STORE_CENSUS=1` (compile and run time) now also counts array element
reads and stores by route: the guarded fast load, the hole arm, the guard
word miss, the runtime fallback, and for stores the in-bounds overwrite, the
inline append and the runtime call. Off by default, and it emits nothing then.
