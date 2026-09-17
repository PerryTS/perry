### Faster

- `String.prototype.replace` with a string replacement (`str.replace(/re/g, "[$&]")`) no longer holds about a kilobyte of memory per piece of its output. On a 2.2 MB subject it now peaks at 108 MB instead of 1,870 MB and finishes in 3.3 s instead of 45 s — less memory than Node uses for the same work (#10411).
