Fixed a crash when a named property is read off a number (for example `function read(x){ return x.options }; read(47)`); it now gives `undefined`, as in Node.
`x[k]` on a number with a dynamic key now finds `Number.prototype` members (it returned `undefined`).
