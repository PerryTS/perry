Added a guarded internal clone for small methods whose numeric parameters are
used as array indexes. Same-module calls reach the clone only when every
selected argument is already proved to be a nonnegative i32; all other calls
keep the public body and its full JavaScript property-key semantics. Guarded
array reads can also follow and revalidate one ordinary forwarding edge before
loading the live slot.

On the `codehz/ecs` 10,000-entity query, an 11-pair contended-host A/B against
the immediately preceding `main` reduced paired medians by 38.0% for read-only
iteration and 57.7% for accumulation, with 0.14% binary growth. All 22
processes passed the query assertions and exact 50,005,000 accumulation oracle;
the cohort is qualification evidence rather than a quiet-host release result.
