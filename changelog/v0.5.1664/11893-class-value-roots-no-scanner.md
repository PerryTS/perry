Removed the per-minor `scan_class_value_roots_mut` root scanner. Class
constructor values are pinned old function objects that fulls already trace as
pinned roots, so the scanner only repeated that work on every copied minor.
The class value now starts with unknown layout (arena-born closures already do),
and the prototype-link store uses the slot write barrier, so a young prototype
or static stored into an old class stays in the minor remembered set for as long
as it is young. Adds tests that write once and then survive several moving
minors (with a sabotage variant that drops the entry) and that a full
collection keeps the statics bag.
