Fold `o.k.length` leaves into a loop region's fact tree (#12485). Inside a
loop region (#10884), a `+` tree over one receiver's field reads already
lowers as a fact tree: bare slot loads, an `fadd` fold checked by its result,
and a generic arm that recomputes the tree in source order when the check
fails. A leaf `o.k.length` was not admitted, so `h += o.a.length +
o.b.length + o.c.length + o.e.length` declined the shared numeric guard
(#10904: a left-leaning chain of property reads) and paid
`js_dynamic_string_or_number_add` at every node, 37% of the str grid's `read4`
cell.

A fact tree now admits `o.k.length` where `o.k` is one of the region
receiver's covered keys. The tree loads `o.k` bare and takes its length from
the value when that value is a string (the SSO and heap arms of
`expr::string_length`) or a live plain Array (the record length guard),
reading which runs no JS. Any other value yields NaN, which fails the fold
check before anything observable has run, and the generic arm reads `.length`
with ordinary property semantics. No calls are added to the fast tree, so the
region verifier's view is unchanged.

str grid medians (instructions per iteration, base -> head): read4 248 -> 93
(18.3x -> 7.2x node; spread 2.42x -> 1.62x), read4_hoisted 22.9x -> 8.0x node
(spread 2.19x -> 1.16x), read1 55 -> 42 (4.0x -> 3.2x node), read1_hoisted
4.2x -> 3.4x node (spread 1.29x -> 1.21x). read4_stmt, inherited and method
are unchanged: their `.length` results reach the add through locals or calls,
which the existing guarded fold already handles.

Tests: `length_leaves_join_one_receivers_fact_tree` (the planner admits the
leaf for one covered receiver only), `summed_string_lengths_fold_in_the_fast_body`
(the emitted IR has the inline string and Array arms, no call in them, and
keeps the generic arm), and `test_gap_12485_region_string_length_tree`
(SSO/heap/astral strings, arrays and Array subclasses, array-likes, a counted
`length` getter read once, #10904's conversion that mutates a later leaf, a
non-numeric `length`, `undefined` receivers, and mid-loop reshaping).
