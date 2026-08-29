### Fixed

- Out-of-bounds `Uint8Array` and `Buffer` reads now preserve `undefined` when
  their value is stored in a plain array instead of silently becoming `0`.
