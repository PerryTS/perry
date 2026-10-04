**HTTP/2 packed settings accept in-range fractions.**

`http2.getPackedSettings()` now checks numeric bounds before truncating valid
fractional values to unsigned integers, matching Node.js while preserving the
31-bit `initialWindowSize` limit.
