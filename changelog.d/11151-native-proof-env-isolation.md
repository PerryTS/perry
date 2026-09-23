Fix the native-proof harness's intermittent `/nonexistent/perry7493` failures
(#11124). Run the environment-restoration sabotage in an isolated child process
so concurrent lock-free compiles cannot observe its deliberately invalid artifact
directory. Check restoration of all three `PERRY_NATIVE_REPS*` variables, both
when originally absent and when holding pre-existing values. Verify the intended
panic payload and require a completion marker from the child so setup failures
or an empty test selection cannot pass silently.

Map the shared helper to its two consuming suites in PR CI so the generic
shared-helper expansion cannot drop both suites behind the named-suite cap.
