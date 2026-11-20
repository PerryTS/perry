### Fixed

- Default-derived constructors now forward their user arguments through a dynamic heritage edge owned by an intermediate constructor-free class. Both direct `new Leaf(...)` lowering and class-value construction call the captured base constructor and then initialize intermediate and leaf fields in order, instead of returning an uninitialized instance.
