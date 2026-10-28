Reduce settled-await scheduling overhead using actual activation ownership and Promise brand, state, metadata and effect proofs. Preserve the existing FIFO jobs, Promise hooks, contexts, activation identity and incremental root insertion barriers. Avoid copying empty AsyncLocalStorage contexts while retaining the canonical native job domain.

Bind the existing private-frame slot barrier to the uniform live IR census and its sabotage matrix.

Share the native enqueue body between both admission routes to avoid duplicating barrier and context scaffolding.

Use one scanned Task publisher for observed and unobserved awaits. Preserve producer notifications and reload the rooted result across a reentrant host wake.
