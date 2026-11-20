Fixed the compiler-output regression CI job so one failing evidence subject no
longer hides every subject after it. The two FP modes now report separately,
and a required build-free liveness check prevents the fail-fast wiring from
silently returning.
