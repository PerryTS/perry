Fix the z8 stream churn test panicking with "attempt to subtract with overflow"
at array/storage.rs in debug builds (#12136). The test installed a hand-picked
scanner subset, so the weak canonical-key table and other caches that the
workload refills kept the old address of a moved keys array; a new `_events`
object was then born with that forwarded array, and `array_front_offset` read
the forwarding record as an array header. The test now registers the
production scanner set. No runtime code changes.
