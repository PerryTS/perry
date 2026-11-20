A `String.prototype` method called with a receiver that is not a string
(`String.prototype.replace.call(obj, "*", r)`, as TypeScript's
`replaceFirstStar` does) no longer makes whole-program facts treat the receiver
as held by unknown code. The method converts its receiver with `ToString`, so
the facts run the receiver's own `Symbol.toPrimitive` / `toString` / `valueOf`
and then the string method, and the receiver's fields stay known.
