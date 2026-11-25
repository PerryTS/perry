On `--target web`, `new DataView(...)` is a DataView, so its `byteLength`, `buffer` and get/set methods work instead of reading `undefined` (#15461).
