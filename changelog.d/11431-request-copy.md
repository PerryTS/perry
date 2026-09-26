Fix `new Request(existingRequest, init)` losing the input's method, body,
headers and request metadata when codegen reduced it to a URL (#10380).
Direct construction and the reflective `globalThis.Request` constructor now
preserve the input value until runtime construction. Explicit init members
override inherited values, including replacing the header list; inherited
bodies retain their raw bytes and transfer their used state. Consumed bodies
and GET/HEAD requests with bodies are rejected.

Add a Node-comparable gap fixture and compiler integration test covering
plain copies, header-only wrappers, dynamic overrides, binary payloads,
mutated source headers, body transfer and the reflective constructor. Register
the new constructor in the GC poll-capable inventory.

Normalize Request subclass handles before copying and transferring their body
state. Guard non-body handle values before reading converted body pointers,
and consume body content-type metadata before nested construction or stream
pulls can replace it. URL-based construction now applies inferred content types
without overwriting an explicit header. Extend regression coverage for these
paths and stream-body overrides.
