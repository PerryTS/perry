Compressed embedded assets (`--platform bun`, files over 4 KB) are now decoded
on first read instead of in the pre-`main` constructor. Frame metadata is still
validated at startup; size and directory queries never decode. A binary
embedding OpenCode's 34 MB web UI no longer spends ~2.8% of its `--version`
startup instructions and 34 MB of resident memory decoding assets it never
reads.
