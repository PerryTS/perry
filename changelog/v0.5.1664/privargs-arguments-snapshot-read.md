Read elided arguments snapshots after one live Array header admission. Remove
the repeated ownership classification in clean_arr_ptr and
array_receiver_gc_tag from the ordinary snapshot read. Bounds, exact numeric
keys, forwarding and descriptor-bearing inputs retain their existing semantics;
the complete getter remains the shared fallback. No cache or registry is added.

Arguments object birth is deferred by the preceding change. The managed Array
snapshot remains necessary until native frame snapshots have precise
statepoint support for their mutable slots.

Bodies which necessarily observe arguments on every successful path retain
entry materialization, using the same proof as their immutable function info.
The proof follows straight-line setup and both branches, rejecting early exits
and unknown control flow. This removes redundant readiness state, intermediate
snapshots and operand rooting while paths avoiding observation retain lazy birth.
