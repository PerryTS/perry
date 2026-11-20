**`MERGE_GUIDE.md`: re-fetch at assembly time, and audit landed trains by
patch-id.** Trains 119/120 shipped the pre-review version of #9731 because the
PR head was fetched at audit time and not re-fetched when the train was built;
that cost a `thread_local!` conversion (a new `tls-budget` failure on `main`,
fixed by #9736), two changelog fragments, and a follow-up gap test. The guide
now says to re-fetch immediately before assembly, and to verify what landed with
`git cherry -v origin/main <pr-head>` — patch-id, because rebase-merge rewrites
every SHA and `git rev-list` therefore reports every landed PR as unlanded.
It also notes `git cherry`'s own false positives when a train reshaped commit
boundaries, and how to confirm one with a file-scoped `git diff`.
