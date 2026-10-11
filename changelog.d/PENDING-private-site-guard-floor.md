- Fix: a private-method site guard read the ShapeId word one MiB below its
  receiver. It offset the biased receiver by a hand-typed `1048580` (the old
  1 MiB handle floor + 4) after the floor moved to 2 MiB, so a call could
  SIGSEGV (OpenCode TUI, `@npmcli/config` `loadDefaults`) or accept a receiver
  without the brand. The guard now uses the shared receiver test and
  `receiver_range::emit_field_ptr`, and a codegen test rejects any emitter
  that offsets a biased receiver by a literal.
