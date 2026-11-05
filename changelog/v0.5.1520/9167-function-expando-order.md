### Fixed

- Function-object expando properties now follow ECMAScript own-key ordering:
  integer-index keys enumerate numerically, while other string keys retain
  property-creation order. Overwriting a property keeps its position, and
  deleting and re-adding it moves it to the tail (#9148).
