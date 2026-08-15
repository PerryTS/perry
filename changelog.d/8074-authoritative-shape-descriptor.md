**`ShapeId` now resolves to exact agent-local object layout facts.**

Each published shape descriptor records the ordered keys array, logical key
count, and live inline-slot count for the current agent. Object allocation and
mutation publish a complete descriptor before exposing its id, and moving GC
keeps descriptors synchronized with live object keys while reclaiming dead
shape metadata. Shape ids are never reused; exhausted callers continue safely
through the existing unstamped-object path. `ObjectHeader.keys_array` and
`.field_count` remain the source of truth, and the runtime and FFI ABIs are
unchanged.
