Add a reproducible canonical wakeup acceptance fixture using the pinned
maintained C backend. Two async calls wait on separate unit streams; a synchronous
sibling wakes them in reverse order. Check wake coalescing before and after
arming, repeated endpoint cleanup and additional-export subtyping. This C-only
probe supplies evidence for the upcoming Perry guest scheduler; it does not
enable async Perry guest output or qualify cooperative cancellation.
