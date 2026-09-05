Fix the native-root provider gate's stdlib fixture to retain and export the
strategy-aware ReadableStream constructor and Response body-init reset helper
used by its compiled app, so it can load as a separate dylib.
