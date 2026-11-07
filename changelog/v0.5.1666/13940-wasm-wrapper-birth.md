WebAssembly host wrappers are born as instances of their constructor: an
exported Memory, Table or Global inherits from `WebAssembly.<Ctor>.prototype`,
`Table.prototype.get/set/grow` operate on their receiver, `Table.prototype.length`
is a getter without a setter, and `new WebAssembly.Table` and
`new WebAssembly.Global` no longer throw.
