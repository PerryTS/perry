**HTTP/2 packed settings validate the initial window bound.**

`http2.getPackedSettings()` now rejects `initialWindowSize` values above
2,147,483,647, matching Node.js while leaving other unsigned SETTINGS bounds
unchanged.
