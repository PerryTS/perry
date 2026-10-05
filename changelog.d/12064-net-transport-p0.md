Add an opaque TransportCore at offset zero of a native transport payload and
link-token dispatch through the payload cell. Each terminal completion owed by
the driver retains the cell; close moves its handle into the driver, and stale
generational completions cannot mutate a reopened transport. Accepted resources
are installed directly into child payloads. Teardown discards link completions.

Rebase P0 onto the current payload ABI: preserve its typed PayloadVTable,
PayloadBuffer support, stream state record and native-this alias. Binding owner
links and reopen use the same cell and vtable, with no second native_state word.
Id routes remain available to subsystems that have not yet migrated; this
prerequisite does not convert ext-net or HTTP connections.

The binding's opaque storage uses MaybeUninit words, so Rust padding and
reserved tail bytes may remain uninitialized. Its constructor checks the ABI
layout before the runtime writes the block. A route switch changes only the
core route, preserving the outstanding multishot read's operation and token.
