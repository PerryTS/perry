### Fixed

Object rest destructuring now preserves enumerable own Symbol properties,
including accessor values and Symbol-only objects, while continuing to omit
excluded and non-enumerable Symbol keys.
