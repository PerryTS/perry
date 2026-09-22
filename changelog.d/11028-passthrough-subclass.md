### Fixed

- Honor `_transform` overrides on classes derived from `node:stream`'s
  `PassThrough`, including aliased, dynamic, and indirect inheritance forms,
  while preserving the default identity transform.
