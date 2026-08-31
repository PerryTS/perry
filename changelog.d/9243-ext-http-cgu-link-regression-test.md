Add an end-to-end regression test for #8907: a program that references a
`cgu.0`-only stdlib symbol (`new Blob([...])`) with no `node:http` import must
link and run.

The bug (released v0.5.1220): #5831 leaked `external-http-client-pump` into
perry-stdlib's `full` feature, which declares `js_ext_http_*` symbols defined
only in `perry-ext-http`. That archive is linked per-program by the compile
driver, and only when the program imports `node:http` — so a full-stdlib link
without an http import left those references unresolved. The release packs
stdlib into one monolithic `cgu.0` object, so any reference that pulls that
member drags the http references in with it. Fixed by #5983 (v0.5.1239), which
drops the pump from `full`; this pins the end-to-end link that the manifest-level
guard in `issue_8587_prebuilt_stdlib_http_isolation` does not exercise.
