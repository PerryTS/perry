Fix the native-root provider gate's stdlib fixture to retain and export the
strategy-aware ReadableStream constructor used by its compiled Response app.
This lets the app resolve the constructor when loaded as a separate dylib.
