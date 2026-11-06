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

Follow-up: retain the creation order of an `ErrorHeader`'s built-in names
and program-defined own properties together in the object's metadata.
Perry lists built-in own names first. This minimal case prints
`[ 'stack', 'message', 'code' ]` on Perry and
`[ 'stack', 'code', 'message' ]` on Node 26.12.1:

```javascript
const e = new Error();
e.code = 1;
Object.defineProperty(e, "message", { value: "m", writable: true, configurable: true });
console.log(Object.getOwnPropertyNames(e));
```

The follow-up should preserve creation order through definition and deletion
without choosing an order from the error's name or code.

## Archive-derived native throw classes

The build appends `throw_class` and `passes` columns to every sidecar row:

```text
symbol  class  throws  throw_class  passes
```

The first three columns keep their existing meaning. `throw_class` bounds
what the runtime itself can throw when an entry is called:
`none < core < passes < open`. The analyzer joins the
four cold leaf symbols it reaches through resolved calls and the existing
seed delegations. `throw_core` adds `core`, `throw_program_value` adds
`passes`, and `throw_open` adds `open`. `rethrow_caught` adds no origin.
A JS re-entry seed marks a call edge into program code. That edge, including
an unresolved indirect program call in the same role, adds no native origin.
The containing runtime function is not cut: its statically known native
callees, including validation errors raised before any callback, still count.
Other unaccounted indirect calls, externs and instruction idioms are open;
the existing audited seed premises remain in force. The collector, poll,
audited nothrow, panic and teardown cuts retain their existing premises.

The existing `fn_info!` realm grammar carries the value declaration. Its
item form preserves the function definition and emits no metadata object or
instructions:

```rust,ignore
crate::fn_info!(#[realm(throws = passes(1))] declare
#[no_mangle]
pub extern "C" fn entry(value: f64, message: f64) -> f64 {
    // The program-value arm throws message unchanged.
    // Other arms may throw through different leaves.
    implementation(value, message)
}
);
```

In this item form the index names a Rust formal argument, starting at zero.
An ordinary realm callable can also carry `throws = passes(0)` on its owning
`fn_info!` record. A stored value can be declared as
`throws = passes(Source.path, 0)`, or several operands can be joined in a
brace block separated by semicolons. That claim is valid only when every
writer stores the stated program argument. A slot that also stores caught
values or runtime-built errors uses `throw_open` until it retains the origin
of its value. `AbortSignal.reason` and Promise rejection storage are mixed
channels; their transport cannot claim a particular program argument.

The generated `realm-v1.json` contains the declarations and their source
provenance. The archive analyzer uses the same source reader. Every path
from an exported runtime entry to `throw_program_value` must cross a declared
producer or an explicit program-body cut; otherwise sidecar generation fails and names the entry.
A declaration whose emitted body reaches no program-value leaf also fails.
An optimized-away or target-disabled body has no path to cover. The separate
raw-`js_throw` graph check examines all runtime nodes, including local bodies.

An own-argument declaration belongs to its declaring entry. A caller or an
alias without its own declaration gets `open` for that origin; the graph does
not guess argument mappings. A stored-source declaration can propagate to
callers. The `passes` column lists `self:<index>` or `<source>:<index>`, separated
by commas; `-` means no operand. An open row can still list a declared operand
for another arm. Sidecar v2 requires both new columns. Legacy standalone
three-column tables parse conservatively with an open native origin bound.

The solver does not consume these columns yet. In particular, a program that
contains dynamic code must treat a caught value as possibly any value dynamic
code can produce; the throw class does not carry that. The compiled statement
evaluator has an explicit `program` seed in `seeds.txt`: its whole body is
program execution, and its interpreted `throw` belongs to dynamic code. That
throw-class-only cut does not alter the existing GC/throws solves. A function value can also enter a local `fn_info!` or native-args
body with no exported sidecar row. The solver's member model needs an explicit
mapping to an analyzed body before it can use a class for that call.

`perry-gc-effects throw-census --archive <runtime.a> [--archive <stdlib.a>]
--out <census.tsv>` writes the exported-entry census and a `.local.tsv`
report of local FnInfo/native-args bodies identified from compiled records.
The diagnostic `without_indirect` column removes unresolved indirect seeds
without adding targets. It is an upper bound on potential delegation work,
never a proof or a sidecar input.
