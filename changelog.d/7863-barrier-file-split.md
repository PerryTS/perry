**`gc/barrier.rs` split at the 2000-line cap.** The evening merges pushed it to
2,157 lines, going red on `main`'s file-size gate. The remembered-set
inspection/drain/clear group moves to `gc/barrier/maintenance.rs` — the #7830
recipe: a cohesive function group, explicit re-export, no logic change. The
moved items' `pub(super)` visibilities widen one level to `pub(in crate::gc)`
(a re-export cannot widen), and the thread-local cold-inventory entries re-key
to the new path.
