Speed up Uint8Array and Buffer reads through ArrayBuffer and subarray views.
Views now keep a resolved backing pointer in their own allocation. Guarded
byte reads use that pointer and the current view length, avoiding repeated
receiver validation and backing-table lookups while preserving out-of-range
`undefined`, detach, resize, aliasing and SharedArrayBuffer reads. Owning
buffers keep their existing inline-storage cache and other typed-array read
lanes retain their existing guards.
