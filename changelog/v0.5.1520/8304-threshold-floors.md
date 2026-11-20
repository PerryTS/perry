### parity: threshold floors ratchet from the 2026-08-17 measured baseline (#8271)

The per-category threshold gate still carried 100% floors for 21 categories
the six-week dark debt broke (18 single-test npm/module categories at 0%,
whose one test each is already triaged in `known_failures.json`, plus the
three legacy buckets at 98.6/81.2/87.8%). Configured exceptions now floor
each at its measured 2026-08-17 rate with #8271 provenance — every floor
only ratchets UP, in the same PR that fixes the debt behind it. In the other
direction, the four stale 0% floors (`parity/http{,2}`, `parity/https`,
`parity/sqlite`) that measured 100% on the same merged report are raised to
100 per the #7582 both-directions doctrine. Verified: the gate exits green
on the merged 8-shard report and red on any further slide.
