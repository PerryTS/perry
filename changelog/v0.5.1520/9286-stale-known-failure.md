The parity triage list no longer suppresses `test_stress_promises`, which now
passes on Linux. A `known_failures.json` entry that outlives its bug silently
absorbs that test's next regression, so the gate requires stale entries to be
removed.
