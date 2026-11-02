Compiler switch `PERRY_VERIFY_FACTS=1` (a compile-time switch, part of the cache key) makes
codegen emit a runtime check at every proof it consumes: elided field reads,
writes and updates (class, key at slot, value kind), proven value and field
calls, the class switch of proven method dispatch, proven static calls, the
dispatch tower's layout proof, and the number test an indexed-owner loop
drops. A miss aborts with the module, line and column of the consumer, the
proof kind, what was expected and what the program held. Off, nothing is
emitted. The facts dump also names compiler-generated CommonJS modules
relative to the cache dir, so two dumps of one program from two cache dirs are
byte-identical.
