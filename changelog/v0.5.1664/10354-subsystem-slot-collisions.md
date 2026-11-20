**Three turnloop sink slots were claimed by two bindings each; the runtime accepted both.**

A binding is a separately linked `staticlib` that registers a completion sink
for its slot, and `perry-runtime`'s `turnloop_net::sink` routes every completion
by that number. `register_sink` stored the pointer and returned `true`
unconditionally — so two bindings on one slot each believed they were
registered, `available()` was true for both, and every completion went to
whichever registered last, which reads the token's low bits as one of its **own**
connection ids.

Three pairs shipped that way, because the P7 database lane and the P5 server lane
numbered from two different ledgers:

| slot | | |
|---|---|---|
| 2 | `perry-stdlib`'s turnloop HTTP client | `perry-ext-pg` |
| 4 | `perry-ext-fastify` | `perry-ext-mysql2` |
| 5 | `perry-stdlib`'s framework server | `perry-ext-ioredis` |

Reaching one needs a program that links both bindings, which is why nothing
caught it — and a fastify app that uses mysql2 is not an exotic shape. It also
just became *more* reachable, since fastify moved onto turnloop in this PR.

Three changes, because one alone would leave a gap:

- `register_sink` now **refuses** a slot already held by a different sink, so a
  future collision costs the loser its transport (it keeps its fallback) rather
  than corrupting the winner's table. Re-registering the *same* sink stays
  idempotent, which is the documented contract.
- The database band moved to 9..=12 (`perry-db-turnloop::subsystem`, the one
  authority for those four), clear of the slots the server and client lanes
  hardcode. `MAX_SUBSYSTEMS` is 16, so there is room.
- `scripts/subsystem_slots.py` (a `lint` step) resolves every slot across every
  crate — literals and the database ledger's named constants — and fails on a
  collision, so the numbering cannot silently drift apart again.

Both guards are sabotage-checked rather than merely green: deleting the
`register_sink` check fails the new unit test on its own message, and restoring
the old 2/4/5 numbers makes the script name all three pairs.
