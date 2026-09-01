### Fixed

- **util.inherits now accepts declared classes in either constructor slot.**
  Perry class constructors are tagged class references rather than closure or
  object pointers; the runtime now stores the Node-compatible super_ property
  on that representation instead of rejecting it as a non-object.

  The prototype link installed by util.inherits is now observable from class
  instances as well: inherited methods resolve through it, and instanceof
  follows the linked prototype chain without creating an incorrect static
  inheritance edge between the constructor objects. Regression coverage spans
  all four function/class constructor pairings. (#9362)
