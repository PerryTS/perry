Fold full collection's old-to-young remembered-set rebuild into its mark pass.
Live old parents now enumerate their strong slots once to mark children and
buffer remembered entries, including malloc children and external owner slots.
The cycle retains its existing buffer through final remark and the sweep-entry
seed drain, then restores it after the remembered-set clear. Weak-target
exclusions and the pre-clear dirty-snapshot coverage repair stay in place.
No new side table, address registry, or arming flag is introduced. Refs #11990.
Remembering mode is selected once per worklist drain, leaving minors' existing
mark loop free of a per-object remembering branch.

Coverage checks unbarriered young and malloc edges, an entry buffered before
finalization, a born-black old parent, stores into an already-traced parent,
equivalence to the reference rebuild, and weak-target exclusion. A test-only
sabotage drops one productive
remembered entry while still marking the child and makes edge coverage fail.
