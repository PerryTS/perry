### Fixed

- Root `Object.defineProperty` / `Reflect.defineProperty` receivers, keys, and
  descriptors across the exotic-probe sequence. A key coercion inside a
  TypedArray or array-length helper could evacuate operands the caller then
  reused, so the define continued against from-space addresses when the helper
  returned `NotTypedArray` or `None`. Operands are now re-read between probes
  and at every allocation boundary (#8507).
