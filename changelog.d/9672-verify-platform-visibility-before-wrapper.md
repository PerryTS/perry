- **The release refuses to publish the npm wrapper until every platform package
  is actually visible in the registry.** npm can report a successful publish
  that never lands: on 2026-09-10 it printed
  `+ @perryts/perry-linux-x64@0.5.1519`, exited 0 and signed provenance into
  sigstore, while leaving the version **staged** — invisible (404, absent from
  the packument's `time` map) and un-republishable (`E409 Cannot publish over
  previously staged version` on every retry, including after an `npm unpublish`).

  The wrapper's existing guard keyed on `npm publish`'s exit status, so it was
  satisfied by that false success and `@perryts/perry@0.5.1519` shipped as
  `latest` with a platform dependency that does not resolve on linux-x64. The
  version could not then be completed from our side at all, and the release moved
  to 0.5.1520.

  Before the wrapper is published, each platform package is now confirmed present
  in the registry (up to 5 minutes each, polling). The check reads the
  packument's **`time` map**, which was the only signal that told the truth here:
  `npm view` returned nothing and the publish exit status returned success for a
  version npm had no record of. A package that never appears blocks the wrapper
  and fails the job.

  The failure mode this prevents is specifically the bad one. A publish that
  fails loudly costs a rerun; a publish that half-lands puts a broken `latest` in
  front of users and burns the version number, because npm versions are
  immutable and the staged slot rejects retries.

  Probe validated against live registry data, including the exact failing case:
  the five packages that really published read visible, and the staged
  linux-x64 reads not-visible.
