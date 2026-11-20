## Add async event-loop counters

The generated executable loop no longer ticks the three timer queues twice per
checkpoint.

`PERRY_MT_PROFILE=1` now reports microtask-drain, timer registration/tick/fire,
event notification, and event-wait counters so scheduler investigations can
distinguish active work from time spent parked.
