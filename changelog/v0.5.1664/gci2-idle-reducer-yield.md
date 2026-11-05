The idle GC reducer no longer runs follow-up full collections on a timer
after a full that freed little. A follow-up now needs the previous full to
have been productive, or old-gen growth since it past the same bar; a full
that freed less ends the sequence until the mutator allocates again (its
collections re-arm the activity arm as before). On the opencode v2.1.2 TUI
this drops the follow-ups that freed 0.48 MB and 0.38 MB per idle period;
on its server, the timed fulls that freed 0.5-2.8 MB each.

Idle compaction now runs the block selection it would make before starting,
and declines when the promised release is under its productivity bar (the
decline waits for the next full's sweep). A pass that released less than half
its promise caps the compaction backoff instead of doubling it.
