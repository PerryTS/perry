### Performance

- `JSON.stringify` of object literals now takes the plain-data emitters
  (#10529). HIR lowers closed-shape literals to anonymous `__AnonShape_*`
  classes, so the emitters' `class_id != 0` gate declined every literal and
  they only ever served `JSON.parse` output. A per-class
  `class_is_plain_record` verdict (one more bit in the existing per-class
  `toJSON` memo entry) replaces that gate. `{hello:"world"}` drops from 4,881
  to 1,702 instructions per call, and the fastify `user` reply payload from
  10,911 to 3,333.
- Each object visited is probed for `toJSON` once, not twice (#10696): the
  parent's lookup (member loop or root entry) is handed to the child's walk
  through a one-shot address token, `TO_JSON_RESOLVED_FOR`. This also reads a
  `toJSON` accessor once, as the spec requires.

### Fixed

- `JSON.stringify` dropped an inherited `toJSON` (from `Object.setPrototypeOf`
  or `Object.prototype.toJSON`) on all-primitive array elements that took the
  shape template's primitive-only loop or the one-field primitive emitter:
  `[p, p]` of re-prototyped `JSON.parse` records emitted their raw fields.
- A `toJSON` result that is a plain record no longer leaks the one-shot
  `SUPPRESS_NEXT_TO_JSON` guard into its first child's `toJSON`.
