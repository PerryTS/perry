`RegExp.prototype.flags` reads `sticky` before `unicodeSets`, as node does (observable through getters). An
`ArrayBuffer`, `SharedArrayBuffer` or `DataView` keeps every numeric key (`ab[-1]`, `dv[1.5]`) as an ordinary
property and reads an index it does not own from its prototype chain.
