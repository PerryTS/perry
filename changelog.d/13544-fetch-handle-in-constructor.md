`Symbol.iterator in headers` and `.constructor` on
Response/Request/Headers/Blob/File/FormData handles were not resolved, and
`Object.fromEntries(headers)` dereferenced the handle id and segfaulted.
Handle ids are now answered through the symbol reader and the fetch-interface
constructor, and fromEntries iterates handle entries.
