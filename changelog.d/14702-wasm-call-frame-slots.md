On the web target `console.log(n, arr.join(","))` and other calls that take an
`arr.join()`, `toReversed()`, `toSorted()`, `with()`, `toSpliced()`, `keys()`,
`values()` or `entries()` argument keep their other arguments, instead of
printing the array in place of an earlier one.
