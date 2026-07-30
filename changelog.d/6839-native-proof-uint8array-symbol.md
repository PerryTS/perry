### Tests

- Strengthened native `Uint8Array` codegen coverage so a proven inline byte
  read must not call either the legacy byte helper or the JS-value fallback
  helper. The disposed-view expectation already present on `main` remains
  covered separately.
