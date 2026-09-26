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
