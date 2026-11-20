Fixed a class static setter receiving the class instead of the assigned value
on the generic property path (`C.x = v`, directly, through a variable or on a
subclass, and `Reflect.set`) and through the reflected setter function
(`Object.getOwnPropertyDescriptor(C, "x").set`), string- and symbol-keyed.
