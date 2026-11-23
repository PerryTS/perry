`Symbol.prototype.description` is an accessor: `Object.getOwnPropertyDescriptor(Symbol.prototype, 'description')` returns its getter, which throws a `TypeError` for a receiver that is no symbol.
