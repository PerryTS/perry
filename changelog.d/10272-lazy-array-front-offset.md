Fix lazy JSON array index probes reading the wrong element after a materialized array is shifted. The probe now uses the array's logical element pointer, matching the general lazy reader and respecting the dense queue's front offset.

A runtime regression exercises three successive shifts, proves the offset-backed storage is active, checks stale-length and out-of-bounds declines, and compares every remaining index against the expected value and general reader. On the unchanged implementation, index 1 after one shift returned 20 instead of 30.
