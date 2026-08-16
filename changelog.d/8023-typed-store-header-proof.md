### Skip GC descriptor probes for plain typed stores

In-bounds plain-double writes to intact typed objects now prove layout
compatibility from the object header and field count, avoiding both
thread-local descriptor-map probes on the common dynamic-store path. Tagged,
pointer-like, and out-of-bounds writes retain the descriptor fallback and its
existing downgrade behavior.
