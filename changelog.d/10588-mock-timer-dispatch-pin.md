Fixed a follow-on to #10447/#10538: a one-shot `node:test` mock timer
(`setTimeout` under `mock.timers`) dropped its own registry pin
(`ScheduledTimerId`) the moment it left the mock queue for dispatch, rather
than after its callback returned. A callback that then scheduled and cleared
more than the registry's 65,536-entry eviction cap's worth of other timers
before finishing evicted its own handle mid-dispatch — the same symptom
#10447 fixed for long-lived real timers, reopened narrowly for the mock
dispatch path. The pin now rides along in the dispatch action tuple and
drops only after `call_timer_callback` returns. Real (non-mock) timers and
mock intervals were never affected. Reproduced against clean `main` with a
new regression test before the fix, confirmed passing after.
