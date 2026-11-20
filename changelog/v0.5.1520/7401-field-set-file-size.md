**Fixed** `lint` was red on `main` — a required check — because #7381 and #7383
pushed `field_set_by_name.rs` from 1990 to 2048 lines, past the 2000-line cap.

The overrun was comment volume, not code: eight `refresh_roots_after_alloc!()`
call sites each carried a multi-line rationale block. The rationale now sits once
at the macro that implements it, which is where it belonged anyway, and the call
sites are bare.

I originally checked this against `origin/main` and concluded it was
pre-existing. That baseline already contained the two merges that caused it. The
correct comparison is against a commit from before the change — 1990 lines.
