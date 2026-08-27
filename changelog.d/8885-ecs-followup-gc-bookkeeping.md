Removed most of the remaining GC-bookkeeping and dispatch overhead on the ECS
command path (follow-up to #8872). General compiler/runtime mechanisms:
branded `number & {…}` intersections and generic/imported type aliases now
lower to their primitive (they were `Any`, so every entity id was dynamic);
inline plain-double fast paths for dynamic compares and truthiness;
beta-reduction of called arrow-literal locals left behind by inlining;
`Map.get` heals stale array-growth forwarding stubs in place; a header-bit
fast lane in `clean_arr_ptr` and every strict array helper; a process-global
address sketch, an object/closure per-object-mask threshold of eight slots, a
death prune for the per-object layout tables and a live-young-record count
that together let the inline allocator skip the stale-layout probe; the
strict store lane, `pop` and `length =` resolve the header once; the
dirty-page and `typeof` caches move to hot TLS; the string-demote tag test
and plain closure-capture reads are inlined; a validated-parent write-barrier
entry; and `length = 0` keeps an all-pointer array all-pointer. On the
upstream `codehz/ecs` "5k entities: 3 commands each + sync" row the compiled
benchmark went from 7.30 ms/op to ~4.7 ms/op (−36%, paired runs on an idle
Mac mini; Node 26.5.1 is 1.76 ms/op).
