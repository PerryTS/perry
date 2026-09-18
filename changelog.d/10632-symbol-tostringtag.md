### Fixed

- **Web/runtime built-ins lack `Symbol.toStringTag` (#10555).**
  `Object.prototype.toString.call(new URLSearchParams())` was `[object
  Object]` instead of `[object URLSearchParams]`, and `x[Symbol.toStringTag]`
  read back `undefined` — the standard cross-realm type check `kindOf`,
  `isURLSearchParams`, `isFormData`, `isBlob`, and lodash's `baseGetTag` use.
  axios 1.19.0 decides how to serialize a request body this way: a
  `URLSearchParams` body was sent as JSON instead of
  `application/x-www-form-urlencoded`.

  Covers `URL`, `URLSearchParams`, `Headers`, `Request`, `Response`,
  `FormData`, `Blob`, `File`, `AbortController`, `AbortSignal`,
  `TextEncoder`, `TextDecoder`, `EventTarget`, `Event`, `CustomEvent` — both
  the brand string and the real `x[Symbol.toStringTag]` value, plus a
  correctly-shaped (`writable: false, enumerable: false, configurable: true`)
  own descriptor on each constructor's `.prototype`. `Map`/`Promise`/
  `ArrayBuffer`/`DataView` are deliberately out of scope (their brand string
  was already correct via a different, structural mechanism — only their own
  property is missing, a separate fix). `Uint8Array` already had a correct
  accessor.

  Root cause: two representations, two gaps. The Web Fetch family and
  `TextEncoder`/`TextDecoder` are small-integer registry handles with no
  brand/property case in `js_object_to_string` or
  `js_object_get_symbol_property`. `URL`/`URLSearchParams` and
  `AbortController`/`AbortSignal`/`EventTarget`/`Event`/`CustomEvent` are
  real objects whose instances are never `[[Prototype]]`-linked to their
  `.prototype` object, so a property installed only there (the issue's
  suggested shape) would never be reached from an instance.

  Fix: a new `web_builtin_to_string_tag` in `perry-runtime` answers both
  `Object.prototype.toString` and `x[Symbol.toStringTag]` from one place
  (reusing the existing `fetch_handle_kind_probe`/structural/class-id
  detectors), and a real descriptor is *also* installed on each
  constructor's `.prototype` for reflection. The directly-affected path
  measured ~4.7x fewer instructions, not slower (the new check runs early
  and short-circuits several later brand checks a `Headers` handle used to
  fall through).

  Validation: new `test_gap_10555_symbol_tostringtag_web_builtins` (brand
  string, property value, and full descriptor shape for all 13 types, plus
  JSON/typeof/non-writability checks) fails on the baseline and matches Node
  on the fix. Two existing unit tests updated (not a regression — the fix
  makes `TextEncoder`'s sentinel handle id meaningful, which one test had
  assumed was generic); `test_gap_url*`/`test_gap_headers*`/
  `test_gap_fetch*`/`test_gap_events_import_4995` (12 tests) unaffected.
