### Fix module identity: readline/promises and WebAssembly.Module wrappers

`require("readline/promises")` now returns the same object as `require("readline").promises`. A `WebAssembly.Module` wrapper has no own properties (`{...module}` copies nothing), and `WebAssembly.Module.exports/imports/customSections` throw `TypeError` for a non-Module argument; the brand is the internal wrapper state only.
