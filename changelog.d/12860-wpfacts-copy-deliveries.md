### Fixes

- Whole-program facts retain Fastify's proofs under the default work bound. Copy propagation shares immutable token sets, batches pending values in arrival order, and sends queued copies only to subscribers present at arrival; later subscribers receive the current value when registered. This avoids repeated prototype/callee relay transfers without changing the analysis model or raising its cap. The completed original model falls from 211.8M to 51.7M work and preserves every fact (312 stable templates).
