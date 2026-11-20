Captured mutable bindings in a lexical activation now share the existing GC
scope record, removing capture-set partitions, separate allocations and closure
capture slots. Loop-body records are still born on each body entry, and TDZ
initialization and dominance checks remain in force. A surviving callback can
retain sibling slots in its own scope.

Guarded array stores can fill holes below the current length when the existing
prototype-chain proof excludes inherited indexed properties. ReadonlyArray
claims and mutable scoped array claims select the existing guarded indexed-read
lowering; receiver layout and canonical index are checked at each use.

Strict identity comparisons can skip the runtime when a structural allocation
proof establishes an ordinary object and both current values are pointer-tagged.
Unproven object claims retain forwarding resolution, preserving aliases after
array growth and avoiding header reads on Symbols or native handles.
