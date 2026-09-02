### Fixed

- Error subclasses no longer receive an enumerable own `name` property during
  construction. They inherit the non-enumerable built-in name from the Error
  prototype chain, matching Node across JSON serialization, own-key reflection,
  spread, `for...in`, and inspection; explicit `error.name = value` assignments
  remain ordinary enumerable own properties. (#9440)
