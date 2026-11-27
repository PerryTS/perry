The `array_record_own_prefix` acceptance test no longer adds an index to
`Array.prototype` or a `return` to `%ArrayIteratorPrototype%`: both writes are
refused since built-in prototypes are locked (DEC 250 (i)), so the program
threw before printing its expected output.
