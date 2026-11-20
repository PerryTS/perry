`filter`, `find` and `findLast` keep the element they hand to the callback in a
root across the call. A callback that allocates could run a moving minor
collection; the element, held in a Rust local, was then pushed (or returned)
at its pre-move address — `Object.entries(o).filter(...)` results read back as
garbage, and a package manager's installs failed at random.
