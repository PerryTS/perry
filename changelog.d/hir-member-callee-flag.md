A builtin function passed as a value inside another call's arguments or callee (`pick(Number.isInteger)(3)`) is the function again, not a read of a global of the same name.
