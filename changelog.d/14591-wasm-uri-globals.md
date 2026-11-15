`--target web`: `encodeURI`, `decodeURI`, `encodeURIComponent` and
`decodeURIComponent` returned undefined, because the wasm emitter had no arm
for them. They now call the host functions, and a malformed sequence throws
`URIError` that `try`/`catch` can catch.
