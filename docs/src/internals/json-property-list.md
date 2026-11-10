# JSON PropertyList reads

An array replacer controls object property reads, not only the final JSON keys.
`js_json_stringify_full` extracts its deduplicated PropertyList and carries it
through nested object and array serialization.

Key extraction coerces entries in array order, including repeated names. A
separate membership set avoids scanning the preceding keys; the ordered list
retains the first occurrence. Average membership work scales with the number
of entries, with extra key storage and hashing proportional to key bytes; this
is not a worst-case or elapsed-time guarantee.

For each object, `stringify_object_with_array_replacer` reads and serializes one
listed property at a time, in PropertyList order. It must not resolve all own
getters into a value snapshot: excluded getters must never execute, and an
empty PropertyList performs no member reads. A getter or nested serialization
can change a later property before that property's read, including adding it.
An included getter's exception stops the walk and propagates to the caller.

The receiver, current key, and returned value remain rooted across allocations
and user callbacks. Each property read derives the receiver from its handle;
no field or keys-array pointer survives an earlier getter. Runtime-private
storage remains hidden, and the whitelist does not filter array indices.

Acceptance tests live beside the implementation in
`crates/perry-runtime/src/json/replacer/property_list_tests.rs` and invoke the
public `js_json_stringify_full` operation. They cover excluded throwing getters,
empty lists, getter ordering, mutations between reads, exception propagation,
and nested/pretty/overflow behavior.

## Exception recovery limitation

A caught throwing getter can leave global stringify state such that a second
`js_json_stringify_full` call on the same object reports a false circular
reference. The identical recovery reproducer fails against the unchanged
baseline as well as this PropertyList implementation. Repairing that global
exception cleanup is a separate follow-up; these tests assert that an included
throw propagates and prevents later getter reads, not that a subsequent call
recovers.
