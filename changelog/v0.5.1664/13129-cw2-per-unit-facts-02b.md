### Changed

- Whole-program facts (`perry-wpfacts`) track more precisely across units: closure contexts are keyed by their captures, a contract parameter holds one argument marker, a closed class constructor holds what its constructions pass, a store effect carries the kinds it stores, and the results of Math functions, global conversions, `Array(n)` and `length` have the one Number kind their operation fixes. A class whose `extends` is a value takes the one class that value holds.
