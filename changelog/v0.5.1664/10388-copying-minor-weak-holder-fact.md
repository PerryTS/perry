Read the copying minor's weak-holder question once per traced object instead of
once per slot. `visit_slot_with_parent` called `weakref::is_weak_target_trace_slot`
for every slot of every object, an out-of-line call that re-reads the parent's
`obj_type` and `class_id` and then rejects on class for every ordinary object.
The per-object form (`weakref::is_weak_holder_header`) already existed: #10182
gave it to the full mark, and the copying minor never got it.

**The fact is read lazily, on the first slot that needs it, not eagerly per
object** — and that is a rule for this collector, not an implementation detail.
Reading it eagerly, once per traced object, regressed every fixture:

    gc3 +1.59%   oldyoung +5.09%   w20000 +1.44%
    w1000 +0.88%   alloc +0.87%    w5000 +0.86%     (instructions:u, min of 5)

with peak RSS +3% to +8% alongside. A great many traced objects — strings,
pointer-free arrays — have no slot to visit at all, and an eager hoist makes
every one of them pay for an answer nobody then asks for. Any future per-object
hoist on this path has to be lazy for the same reason.

Measured on six GC fixtures, instructions:u min-of-5: gc3 -0.99%, w5000 -1.61%,
w20000 -1.37%, oldyoung -1.10%, w1000 -0.81%, alloc flat. Peak RSS within 0.1%
on all six and max pause better or flat on all six. On a control that isolates
the slot term (60k records whose fields all point at one shared object, against
the same records holding doubles, so the mutator difference cancels between
arms), the pointer-slot-attributable instruction count falls 4.4% at 2 slots per
object, 5.9% at 8 and 6.3% at 16 — 25.5 instructions per pointer-slot visit,
which cross-checks against the 6.7 cycles/slot of self time the profile
attributed to `is_weak_target_trace_slot`.

The companion hoist for `barrier_parent_needs_remembering`'s generation clause is
deliberately NOT included. It measured as the half that makes a
one-pointer-slot object pay (oldyoung +0.198% with it, -1.10% without), and no
sabotage of it could be made to fail: sticky dirty-page coverage carries an
old-to-young edge independently of the remembered-set re-insertion, so forgetting
the fact changes nothing observable. It needs its own witness first.

`gc/copying.rs` loses the slot visit to a new `gc/copying_parent_facts.rs`, which
the 2000-line file lint required and which leaves `copying.rs` smaller than before.
