### Fixed

- Validate rooted `net.SocketAddress` constructor options before reading fields.
  Omitted options, `undefined`, and ordinary, custom-prototype, `Date`, and `Buffer`
  option bags remain accepted; `null`, other primitives, functions, and arrays
  now throw `TypeError` with code `ERR_INVALID_ARG_TYPE`.
- Check the central runtime `typeof` classification before the existing `IsArray`
  check, keeping subnormal numeric options out of legacy pointer inspection.
  The isolated subnormal baseline silently accepted the value; no baseline crash
  was reproduced.
- Add a 13-case public constructor regression covering accepted and rejected
  options. This is a constructor-admission fix, not completion of NET parity:
  remaining parity failures, a timeout, and three unrun cases are outside this
  change.
