Per-unit facts: a store into another unit's literal reached through a result
(`get().p = o`, then a third unit writes what `b()` reads back) no longer
leaves the stored object's field read-only per unit while the whole program
has it written. Two gaps made the reader read ⊤ (or a stale `null`) for a
field its owner had described, so the reader's store reached no object:

- an owner's literal layout named only by its value contracts was never
  published under its display, so the only entry readers took
  (`Gen::other_layout`, DECISIONS 106) was a forwarder's copy from an earlier
  round, which the forwarder read back and forwarded again. The owner now
  publishes it, described, as DECISIONS 132 does for classes;
- joining two descriptions of one layout (`ClassContract::join`, across
  rounds and owners, and `widen_stand_in` inside one solve) turned a field
  the two describe differently into an unknown value. The join now keeps
  what each side says (plain facts and field functions as a value contract),
  and the reader builds every description it is given.

Red on main in three forms (a direct store, a DECISIONS 148 method parameter,
a decision-13 closed parameter); tests in `unit_mode/shared_values.rs`. Per
unit also proves more where the join made fields ⊤: the CommonJS fixtures
prove all whole-program sites (22/22, 28/28; were 7 and 13).
