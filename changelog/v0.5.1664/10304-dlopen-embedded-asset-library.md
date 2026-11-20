`dlopen` on an embedded asset library now works.

`import lib from "./libfoo.so" with { type: "file" }` lowers to a `$perryfs/<name>`
virtual path, and a bun-compiled binary hands exactly that path to `dlopen`. The
dynamic loader only accepts a real filesystem path, so the load failed with
"cannot open shared object file" — OpenTUI's renderer is loaded this way.

The virtual path is now translated inside `open_library`, which both
`dlopen_value` and `node_dlopen_value` reach, rather than at one call site. The
embedded bytes are materialized once per virtual path into a per-process
directory created with `O_EXCL` and mode `0700` under an unpredictable name, and
the file itself is created `O_CREAT|O_EXCL` at mode `0700`, so an existing file
or symlink at that name is refused rather than followed and the bytes are never
briefly readable by another user. The file is keyed by a digest of the full
virtual path, not the basename: two embedded libraries can share a basename
under different `$perryfs/` prefixes, and naming by stem alone let the second
materialization overwrite the first.
