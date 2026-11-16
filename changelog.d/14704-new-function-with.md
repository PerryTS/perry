Code built at run time with `new Function` or indirect `eval` can use `with`
statements (#13962). This covers lodash's and underscore's `_.template`
default output. Lookups and assignments go through the object, honor
`Symbol.unscopables`, and call methods with the object as `this`, including
through `(f)()` and `f?.()`. A `var` in an interpreted function now shadows a
binding of the same name in an enclosing scope instead of writing to it.
