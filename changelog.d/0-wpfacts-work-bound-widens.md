Whole-program facts no longer drop every fact when the solve passes its work
bound (`PERRY_FACTS_MAX_WORK`, default 200M). Past the bound a call that would
start a new calling context runs the function's shared instance instead, so
the solve reaches a coarser fixpoint and finishes: facts stay proven where the
merged contexts still agree, the rest become unknown. A program under the bound
gets exactly the facts it got before. The widened solve is bounded too: past
1000 times the bound it stops, claims nothing and says so in the facts report
("widened past the work bound, then OVER 1000 times it: no facts"). Per-unit
facts keep the old rule (past the bound, no facts).
