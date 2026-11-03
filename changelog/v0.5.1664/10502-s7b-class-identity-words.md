Class-table retirement S7 phase 2: a class identity now has a word in its shape.
Registering a declared class publishes an unbuilt word, building its declared
prototype or replacing `F.prototype` stores the holder there, and workers seed
a pointer-free unbuilt word. Read sites, `then`/`toJSON` probes and
inherited-method memos compare that word instead of the process-wide lookup
generation, which is deleted. A read-site entry records the word's stable slot
when it is proved, so each hit costs two loads.

An object literal's anon-shape class id has no class surface. It never builds a
declared prototype or publishes a class word, so `({}).constructor === Object`
holds and the literal stays on ordinary property lookup.

The remaining vtable readers (lookup-in-chain, method owner, member checks,
declared-parent walks, the then-probe registry check and the typed-feedback
vtable match) are replaced by holder-shape reads. The vtable is only
materialization input. The direct-method shape guards drop their unused
guard-slot ABI argument. `retired_symbols_tests` covers every deleted name.
Refs #10502, #12240.
