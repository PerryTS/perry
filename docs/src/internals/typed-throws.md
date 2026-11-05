# Runtime throw provenance

Runtime Rust code chooses a throw leaf from the producer of its value:

| Leaf | Producer |
| --- | --- |
| `throw_core(CoreError)` | A fresh `ErrorHeader` built by a core error constructor |
| `rethrow_caught(Caught)` | A value read by a catching funnel |
| `throw_program_value(f64)` | An argument or stored value supplied by the program |
| `throw_open(f64)` | A hook-built object, marker, or channel mixing origins |

`catch_thrown` retains the caught carrier; `catch_js_throw` exposes its value
for consumers such as Promise rejection and callback arguments. Reading a
carrier's value does not establish a new origin. Mixed channels stay open
until their producers and existing storage retain the distinction.

A carrier does not root its value. Use its rooted form when the existing
control flow keeps a value across allocation, and preserve the existing
root scanners and write barriers when retaining it in an owned slot.

The four cold, never-inlined leaves share the exception transport. They
preserve the thrown bits and the construction, property-write and callback
order. The private, unmangled `js_throw` symbol remains the compiled-code
entry. The JS body funnel lint forbids runtime, stdlib and extension crates
from calling or redeclaring that raw entry; unit fixtures use a test-only
probe. These carriers record throw provenance; they do not derive program
facts from Rust or TypeScript types.
