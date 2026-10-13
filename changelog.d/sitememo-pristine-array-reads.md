### Changed

- An inherited or absent read on a plain Array takes the typed
  `Pristine(Array)` body: while `%Array.prototype%` and `%Object.prototype%`
  are unmodified (`builtin_proto::PristineReceiverFact`, one byte load), the
  site's array entry is matched by its own token, so the read no longer
  resolves `%Array.prototype%`, its link flags and its property bag on every
  miss. The entry's holder and epoch proofs are unchanged; a modified
  prototype falls back to the full receiver proof.
