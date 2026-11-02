Regression test for #13485: an own property written on a built-in iterator
(array, Map, Set, string, typed-array, `matchAll` and iterator helpers) no
longer clobbers its internal state, so `next()` keeps advancing and the
property reads back like in Node. Already fixed on `main`; this pins it.
