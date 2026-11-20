Speed up Uint8Array and Buffer reads through ArrayBuffer and subarray views.
Views now keep a resolved backing pointer in their own allocation. Guarded
byte reads use that pointer and the current view length, avoiding repeated
receiver validation and backing-table lookups while preserving out-of-range
`undefined`, detach, resize, aliasing and SharedArrayBuffer reads. Owning
buffers and views with immutable synchronous byte parameters resolve their
storage once at entry when indexed in loops. Other sites keep their guarded
admission paths, and
other typed-array read lanes retain their existing guards.
