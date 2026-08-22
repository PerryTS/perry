### Fixed

- Web `TextField`, `SecureField`, and `TextArea` callbacks now receive their
  entered text as strings instead of numeric `NaN` values. The WASM closure
  bridge converts browser values directly to its BigInt ABI without an
  intermediate NaN-boxed JavaScript number. (#8584)
