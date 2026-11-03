Shape guards on a class of the module being compiled now compare the receiver
against the class's static ShapeId as an immediate instead of loading the
id the shape mint stored at module init. Constructors still stamp the id the
mint returned, so when the mint declines the static id no object carries it and
the guard only takes its slow path. Guards naming a class of another module
keep loading that module's id.
