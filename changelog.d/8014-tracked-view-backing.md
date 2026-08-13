**Fixed: tracked native views could retain a stale cached backing pointer across collecting operands (#7640).**

ArrayBuffer and native-arena views now leave the cached-pointer fast path before
evaluating an index or stored value that can collect or re-enter user code. The
rooted runtime fallback resolves the view's current backing after that window,
while fresh inline Buffer and TypedArray storage keeps its existing fast path.

Computed-access IR regressions now trace each protected receiver and key through
its exact root slot, and the call-free masked-window invariant is documented so
those loops retain their zero-root direct loads.
