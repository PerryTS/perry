Fix inherited static calls through a function superclass using the original class receiver. Object-literal methods now use the existing Function.call/apply rebinding mechanism before invocation, so nested arrows capture the class rather than the superclass's prototype. This removes the receiver-argument-only assumption without adding a dispatch path or cache.

Regression: test-files/test_gap_ocrun2_service_receiver.ts matches Node and Bun; the base prints the prototype key for the class call. OpenCode's Context.Service.use is the witness.

Review: all generic explicit-this calls now rebind concise-method captures in the shared value-call dispatcher. Remove the per-site rebinds, root the complete call across the clone, and pass the class receiver when invoking static fields. Arrow and bound statics retain their own binding.

Static-field callees now use the existing property Reference call lowering, preserving P in P.f() and the subclass in inherited calls. This removes the dedicated static-field plain-call emission.

Rebased onto main’s single explicit-this forwarder: generic receiver-bearing value calls enter that same operation. Removed the reviewed duplicate value-call rebind/rooting helper; main already covers call/apply/bound/Reflect.apply rebinding, GC handles and in-place call/bind arguments. Static-field receiver retention and inherited function-superclass receivers use the same forwarder.

Semantic rebase: use main's single explicit-this forwarder for every generic
receiver-bearing value call. A captured receiver that already matches skips
the clone. This covers plain method calls and prevents call/apply forwarding
from cloning an already-prepared method twice. Callback receiver regressions
are covered in TypeScript and CommonJS forms.
