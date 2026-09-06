The first paired admission stopped before trials because initial one-minute
load was 2.59, above the 2.5 threshold. The controller exited, its lock was
released, and no paired result directory existed. A subsequent read-only check
found load 2.07, no lock and no competing workers before starting the retry.
This rejection does not invalidate the preceding qualified full matrix.
