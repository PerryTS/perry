### Fixed

- **`JSON.parse` could read its input after the source string moved.** Both
  `js_json_parse` and `js_json_parse_result` derived a byte slice from the
  source `StringHeader`, then called `gc_check_trigger()` — a deliberate
  collection point that sheds parse-churn garbage between iterations — and only
  *then* pushed the string's GC root and suppressed collection. An evacuating
  minor at that trigger moved the string, leaving the parser reading retired
  from-space for the entire parse.

  The root now precedes the trigger, and the slice is re-derived from the rooted
  value afterwards. Ordering is the whole fix: pushing the root *after* the
  collection roots an address the collector has already moved away from, so
  re-deriving from that slot returns the same stale pointer — verified by trying
  exactly that first and measuring no change.

  Invisible from output, because evacuation copies rather than zeroes and the
  stale address still held the right bytes. Found with
  `PERRY_GC_PROTECT_FROMSPACE=1`, which faults on the first `peek()`.

  Closes **8 of the 31** remaining quarantine catches (#7341) — the largest
  single cluster.
