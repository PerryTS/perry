`require('stream')` and `import Stream from "node:stream"` are now the legacy
`Stream` constructor, as in Node, instead of a separate namespace object
(#10430, #10431). Before, `x instanceof Stream` threw "Right-hand side of
'instanceof' is not callable" for both forms (node-fetch's `body instanceof
Stream`), `Stream !== NamedStream`, and nothing inherited from EventEmitter:
`require('stream').EventEmitter` was undefined, so redis's
`class ClientSideCacheProvider extends stream_1.EventEmitter` threw "Class
extends value is not a constructor" at module init. `new Stream()` also built an
empty placeholder with no `on`/`emit`, and `class X extends require('stream')`
instances had no EventEmitter methods.

Root cause: `cjs_default_export_value` had no `stream` arm, so the CommonJS
module value fell back to the namespace object; the HIR lowered the default
import's value to the bare `NativeModuleRef("stream")` (only its `typeof` was
folded to "function"); and `attach_stream_legacy_prototype` never linked
`Stream`/`Stream.prototype` to EventEmitter or hung the exports on the
constructor.

Fix: the CommonJS value and the default binding's value both resolve to the
named `Stream` export. The constructor carries every module export as an own
static (Node's own-key order, `Stream.Stream === Stream`), and gets Node's two
`ObjectSetPrototypeOf` edges (`Stream` → `EventEmitter`, `Stream.prototype` →
`EventEmitter.prototype`). `new Stream()` builds an instance of
`Stream.prototype`, and a dynamic `extends` of `Stream` gets the EventEmitter
parent edge and EventEmitter init on `super()`. The attach now roots the
constructor and prototype across its allocations. A namespace import stays the
namespace object, and member reads and calls on the default binding keep their
static lowering.

Validation: `test_gap_10430_stream_module_constructor` differs from Node on
7661bc05fe and matches it here, in both no-auto and auto-optimize modes. HIR and
runtime unit tests were added. The full gap suite matches the snapshot, with the
same 6 known mismatches as the baseline. The stream + events node-suite is
868/872 on both baseline and fix, with zero per-test deltas. Stream data paths
are flat in `instructions:u` (+0.05% and +0.22%). `instanceof Stream` alone is
+0.56–0.71% (median of 7), within the ±1% band. Startup cost is +1.1 M
instructions, paid once when the constructor is first minted.
