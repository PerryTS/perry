Fixed `PERRY_GC_VERIFY_EVACUATION=1` re-entering the thread-local malloc
registry while it validated malloc-backed object fields. Diagnostic heap walks
now snapshot malloc headers before validation, so copying-minor verification can
run with a populated side table instead of panicking on a nested `RefCell`
borrow.
