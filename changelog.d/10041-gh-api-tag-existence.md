- **`create-release`'s tag check now gates on `gh api`'s exit status, not its
  stdout.** On a 404, `gh api` prints its error JSON to **stdout** and exits
  non-zero, so `existing=$(gh api ... || true)` captured
  `{"message":"Not Found",...}` instead of an empty string. The "tag exists at a
  different SHA" branch therefore fired on the **normal** path — an absent tag —
  and aborted v0.5.1520 with the self-refuting message
  `tag v0.5.1520 already exists at {"message":"Not Found",...}, not the
  candidate 381045a87`.

  The check now runs `gh api` for its exit status first and only reads
  `.object.sha` when the ref really exists, plus rejects anything that is not a
  hex SHA. Verified against the live API in all three states: absent → create,
  present at the same SHA → reuse, present at a different SHA → abort.

  This one is worth a note on method rather than mechanism. The same commit that
  introduced this bug also introduced the release-notes cap, and the cap was
  tested carefully against the real 2.8 MB notes while this branch was never
  exercised at all. A test that covers the half of a change you were thinking
  about is not coverage of the change.
