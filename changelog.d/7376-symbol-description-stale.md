### Fixed

- **A `Symbol`'s description was stored into the header already stale.**
  `alloc_symbol` calls `gc_malloc` — a collection point — and then writes the
  description pointer its caller computed *before* that call. An evacuating
  minor moves the description string, so a live `SymbolHeader` holds a retired
  from-space address and `js_symbol_to_string` faults reading it through
  `str_from_header`.

  The description is now rooted across the allocation and re-read, the same fix
  as `RegExpHeader::flags_ptr` (#7374).

  Closes **6 of the 31** catches in #7341, not 3: the same stale field is read
  by two different helpers. `js_symbol_to_string` reaches it directly, and
  `infer_symbol_function_name` reaches it through
  `js_object_literal_infer_computed_function_name` — which had been triaged as a
  separate cluster until the fix closed both.

  **A related gap is left open deliberately, and is worth knowing about.** The
  header is allocated `GC_TYPE_STRING`, whose payload the collector treats as
  opaque — so a fresh (non-registered) symbol's description is never marked or
  rewritten after construction. The existing comment says so: *"kept alive
  through the SYMBOL_REGISTRY (for registered symbols) or not at all (for fresh
  symbols — in practice they live for the duration of the program, which is fine
  for test workloads)"*. This change makes the stored value correct; keeping it
  alive for the symbol's lifetime is a separate fix, tracked in #7341.
