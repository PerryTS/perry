### Fixed

- Preserve parsed URL state when constructing classes that extend `URL`, including subclasses with an implicit constructor and constructors that explicitly call `super(...)`. URL subclasses now retain their own prototype and fields while exposing the expected URL properties and `instanceof` behavior.
