**Two collector passes kept objects alive that nothing could reach (#9628, #9629).**

**Block persistence ran on cycles whose root set is complete (#9628).** The
pass force-marks every object in a nursery block that still holds one
reachable object, and pushes each onto the trace worklist so their children
are retained too. It exists for one hazard: an object the mutator holds only
in a register across a collection, whose block survives but whose
individually-swept malloc children do not, so the mutator later reads freed
memory (#43/#44). That hazard needs a root set the scan cannot see into.

Its early-out skipped the pass only when the conservative stack+register scan
had run. #7558 then removed that scan from `manual_gc_collect_now` on the
argument that a call is a statepoint and everything live across it is rooted
by codegen — so every explicit `gc()` started running the pass instead of
skipping it, and the comment above the early-out went on naming `gc()` as a
case that skips. Measured on a fixture that drops 11,000 objects and calls
`gc()`: 13,364 objects force-marked, none reachable from a root.

The condition now asks the question the pass is actually about, "can a
root-invisible register hold a live object here?", and answers no in three
cases: the conservative scan ran (as before), no generated frame is live on
this thread (verbatim the completeness argument `pressure.rs` already makes),
or the collection is an explicit `gc()` (#7558's argument). Safepoint-driven
collections still run the pass; they are equally provably redundant, but the
cycle has no signal for "at a safepoint" yet and this change does not invent
one. `PERRY_GC_BLOCK_PERSIST_ALWAYS=1` restores the old behaviour.

**A full mark-sweep marked the remembered set as roots (#9629).** For a minor
that is necessary: it deliberately does not trace the old generation, so
old-to-young edges genuinely are roots. A full trace visits the old
generation from the real root set, so every young object a LIVE old object
points at is reached anyway, and the dirty-page scan validates only that the
owner is a plausible pointer, never that it is live. The one thing marking
added was the young objects reachable exclusively from DEAD old objects.

Full traces now take the snapshot but mark nothing from it. Taking the
snapshot is not optional and this is the trap in the change: that call is
what lazily arms the write barrier and reconstructs the log from the heap, so
skipping it as well would drop old-to-young stores, which fails in the far
worse direction.

Both tests fail when their own fix is reverted, which is how they were
checked: the first reports 20,000 resurrected objects, the second a non-zero
`newly_marked` on a full cycle.

**Measured effect, stated honestly.** On the compiled claude-code TUI at idle
(Linux, three full collections), block persistence force-marked **0** objects:
its recent-block window happened to hold no dead neighbours, so #9628 buys
nothing there and the 13,364 figure above is a fixture result, not a TUI one.
The remembered-set marking on those same three full cycles marked **845, 800
and 800** young objects as roots. After the fix a full trace marks none of
them, and every one that is genuinely reachable is still reached from the real
roots; the retention actually removed is the subset reachable only from dead
old objects, which this telemetry does not separate. Both changes are
correctness and hygiene rather than a large idle-memory win.
