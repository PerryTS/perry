Code built at run time with `new Function` or indirect `eval` can use `with`
statements (#13962). This covers lodash's and underscore's `_.template`
default output. Lookups and assignments go through the object, honor
`Symbol.unscopables`, and call methods with the object as `this`.
