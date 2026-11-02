Build and distribute source-matched WASI 0.3 runtime bundles. Host archives for
Linux x86_64, macOS ARM64 and Windows x64 include perry-wit and the optional
native component engine, co-built with their matching compiler/runtime/stdlib
feature union. Require isolated installed P3 command and typed plugin execution
before staging those archives. Older release sources retain their existing
P1/P2 bundle behavior.
