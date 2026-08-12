## GC ratchet baseline follows the reviewed collector model

The internal Perry-vs-Perry GC ratchet is re-pinned after the merged live-byte
accounting, nursery allocation, and bounded untraced-promotion changes. The new
artifact records the accepted grow-then-churn transition explicitly, including
its additional promotion work and pinned-host footprint, without weakening any
probe or tolerance.
