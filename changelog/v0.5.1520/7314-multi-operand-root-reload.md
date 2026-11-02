Follow-up to #7311. An instruction carrying **two** stale operands only had one
of them reloaded.

`root_reload`'s apply loop renamed an operand and then inserted its reload, one
rewrite at a time. For two rewrites on the same instruction the first insert
shifts the consumer down by one, so the second `rename_operand` addresses the
freshly-inserted reload instead of the consumer — it matches nothing, the second
operand stays stale, and a load nothing consumes is emitted.

`js_object_assign_one(receiver, value)` is exactly that shape (`index_set.rs`
lowers `object` before `value`, so both can be slot loads), and it is the
population #7311 reports going 137 -> 0 on the dependency-scale corpus. A
per-instruction count can reach zero while an operand is still stale, so the
headline number was measuring less than it appeared to.

Rewrites are now grouped per instruction: every operand is renamed against the
original instruction, then that instruction's reloads are inserted.
