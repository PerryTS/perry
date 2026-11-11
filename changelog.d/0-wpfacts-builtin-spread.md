Whole-program facts: a spread (`{...x}`) or `Object.assign(t, x)` whose
source is a builtin object the model fills (a `JSON.parse` value, an
`Object.keys` or `Reflect.ownKeys` array, a primitive box, an
`Object.groupBy` or `Object.fromEntries` result, a property descriptor)
now copies that object's own values into the target, where it used to make
every key of the target unknown. Other builtin objects (Date, Map, RegExp,
typed arrays …) and values from unknown code stay unknown, as before.
