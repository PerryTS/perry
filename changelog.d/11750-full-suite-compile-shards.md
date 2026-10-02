Distribute full-tier compile work across bounded jobs after current runs hit
their timeout limits. The auto-optimize gap suite now uses twelve shards.
Compile-smoke assigns the complete top-level TypeScript inventory exactly once
across four round-robin shards, with at most two smoke shards running at once.
The existing compiler invocation, platform exclusions, retry policy and failure
markers remain intact; every shard still contributes to the aggregate result.
Invalid or empty selections fail closed, and smoke error artifacts are keyed
by shard. Local selection checks cover all 2,067 current files; actual full-tier
CI must establish the resulting wall-time margin.
