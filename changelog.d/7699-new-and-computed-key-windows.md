### Constructor arguments on `lower_new`'s non-class branches, and the computed-key windows (#6986, #7640)

#### `lower_new`'s non-class branches (#6986)

`lower_new_impl` has wrapped its arguments in a rooted scope since #6983, but
three branches early-`return` before the main class loop ever adopts into it, so
the scope was open around them and empty:

* **`new Readline(…)`** — `output` sat in a bare SSA register across `options`'
  lowering *and* across every trailing argument's, before
  `js_readline_promises_readline_new` read it;
* **`new <importedFn>(…)`** — `func_double` across every argument, and argument
  `i` across the arguments after it, into `js_new_function_construct`;
* **`new Function(…)`** with a dynamic body — the same loop, into
  `js_function_ctor_from_strings`.

All three now adopt into the enclosing `RootedGroup` **as each operand is
produced** and re-read at the call. Interleaved, not appended: rooting a
finished list publishes an already-dangling argument 0 to the scanner, which
turns a silent wrong answer into a SIGSEGV (#6969's trap, restated at the new
helper). The `undefined` fillers for absent readline arguments stay literals, so
`new Readline()` still costs no slot.

`lower_js_args_array` is no rescue and is not touched: it is a plain
`alloca_entry_array` pack with no `js_shadow_slot_bind`, so it copies whatever
bits it is handed, stale or not. The repair has to happen before it runs.

**Not closed here:** `lower_call/builtin.rs`'s multi-argument constructors —
about 22 arms (`Uint8Array`/typed-array views, `DataView`, `RegExp`, `Event`,
`CustomEvent`, `DOMException`, `Console`, `SuppressedError`, `AsyncResource`,
`Blob`, `File`, the `node:sqlite` trio, `CronJob`, `Response`, `Request`, the
stream constructors). `lower_builtin_new` takes no rooting context at all, so
they cannot be reached from a `new.rs`-level fix; each needs its own
`with_operands_rooted`. Left on #6986 with the inventory.

#### The computed-key windows (#7640)

**Section D — the free one, all six sites.** `unbox_str_handle` is not a mask: it
calls `js_get_string_pointer_unified`, which materialises an SSO value into a
fresh heap `StringHeader`, i.e. one allocation per SSO unbox. Six sites computed
the receiver's raw untagged pointer *first* and called it *second*, so a raw
`i64` no root can name crossed a potential collection point — #7280 taxonomy (a),
which `crate::rooting` structurally cannot express. Four are pure statement
swaps at zero runtime cost (the same two instructions, in the other order):
`index_get.rs`'s dynamic-string-key arm, `index_set.rs`'s `globalThis[k] = v`
arm, its `arr[stringKey]` arm and its dynamic-string-key arm. Two are
cross-block — the handle was computed in the arm's entry block and used two
conditional branches later, inside the string sub-block — and are repaired by
re-deriving the handle in that block, below the key unbox.

**Section B — the receiver across the key.** Seven arms of `index_get.rs` lowered
a receiver, then lowered an *unconstrained* index (`o[f()]`), then used the
receiver, with no rooting decision at all: the string receiver `s[f()]` (a heap
string, very much movable), the `recv_unknown` inline dyn-typed-array get, the
`is_array_expr && !is_numeric_expr` arm, `numeric_index_needs_runtime_key`, the
number-context `lower_unknown_local_index_get_for_number_context` tail, and both
`Expr::SymbolFor` arms (`js_symbol_for` interns, so it allocates). Each is now
one `rooting::with_operands_rooted` group over `[object, index]`, which is what
the file's two already-guarded arms are. Where the index provably cannot collect
— a literal, a plain local — `operand_protection` answers `Reuse` and the group
emits nothing, so the proven-index fast paths are untouched.

`test-files/test_gap_7640_computed_key_windows.ts` covers all of it — the
side-effecting index on a string receiver, non-numeric and unproven-numeric
computed keys on an array, a symbol key, SSO string keys read and written, an
`a[stringKey]` write, `globalThis[k] = v`, and the byte-read fast path — and
matches node 26.5.1 byte for byte.

**Still open on #7640:** section A's typed-array store arms and the bounded-index
array store, section C (the unsubstantiated statepoint claim above the
class-field store), and section E's callees.
