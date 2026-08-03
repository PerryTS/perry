### Fixed

- **`PERRY_SHADOW_STACK=0` no longer silently disables statepoint roots.** The
  shadow stack's root-set *analysis* and its *lowering* were one knob, so
  combining the bisection knob with `PERRY_STATEPOINTS`/`PERRY_RS4GC` switched
  the analysis off and left the statepoint lowering with nothing to lower — a
  binary with no precise frame roots at all, no `__perry_gcmap` section, and
  correct output right up until a collection freed a live object. #7332 made the
  combination a hard error; this splits the predicate so it is *expressible*
  instead.

  The eight sites that build the slot map now gate on
  `precise_root_analysis_enabled()`. The lowering choice was already independent
  inside `LlFunction`, so nothing else moves. Under statepoints the knob is now
  provably inert: identical 885-byte root map and identical `__text` with and
  without it, asserted by a new `gc-native-roots` step that also fails if the
  probe roots nothing (which would make the comparison vacuous).

  This changes no default and deletes nothing. It is the prerequisite for the
  shadow-stack lowering ever being *removed* rather than merely switched off in
  a configuration nobody could run.
