Restore release-mode runtime unit-test compilation after the layout and GC diagnostics changes. Pure verification/formatting helpers are available to test builds, while instrument-dependent positive tests use the existing instrumentation cfg. Production cfg behavior is unchanged.

Update static-method fixtures to JsFunctionInfo and isolate the array-literal proof witness. Delete the regression for the deliberately removed INT32 constructor representation. The Linux test harness resolves its optional main symbol without the conflicting weak data declaration.

The native-alias read regression remains enabled and is separately owned by manager decision.

Initialize both threads in the timer thread-exit integration fixture through the production GC/layout startup sequence before allocating Timeout layouts. The fixture retains the earlier-deadline, teardown and surviving main-thread control checks.

Model promoted old parents with the required TENURED header flag in pinned-root fixtures; keep the minor and evacuation child-survival checks.
