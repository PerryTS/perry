### Fixed

- `JSON.parse` syntax errors now report the invalid character's UTF-16 position and its line and column. This also covers malformed documents parsed by the iterative deep-document path. (#10882)
