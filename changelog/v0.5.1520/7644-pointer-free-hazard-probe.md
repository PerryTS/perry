**test(gc): the POINTER_FREE trace-skip hazard probe that faults (#7635)**

#7635's mystery — forcing `POINTER_FREE` on pointer-bearing parse records
strands nothing under any instrument — was the probes, not the instruments:
`JSON.parse` of a non-tiny blob is lazy (#7499), so probes that only read
records back after the churn materialized the cohort after the collections
ran. The identical shape with one pre-churn touch loop faults immediately
(SIGSEGV under default cycles; a precise `[gc-fromspace-protect]` report
under zeal+protect).

`gc/tests/layout_pointer_free_hazard.rs` plants the hazard at unit level
with no env knob: a lying `saw_pointer=false` finalize strands a field-only
young string — asserted on both halves (slot unrewritten AND child in
poisoned from-space) with subject-liveness (the object must have moved) so
neither arm passes vacuously; the truthful twin proves evacuation+rewrite.
Standing rule recorded: parse-cohort GC probes must defeat the lazy tape or
state that they exercised the lazy path.
