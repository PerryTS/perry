### Faster

- Regex calls spend about 9% fewer instructions. The GC safepoint check each search ran now happens on one search in 64, which measurement showed was doing no collection work on the other 63 (#10166).
