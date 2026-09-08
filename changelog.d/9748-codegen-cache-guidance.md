Make the codegen environment-variable registration failure name the registry
file, the input and exclusion declaration anchors, and the command to rerun.
Document the cache-registration requirement in the contributor guidance and
beside the OnceLock reader pattern so new switches are registered when added.
The existing missing-input and stale-exclusion checks remain enforced.
Also register `PERRY_CONCAT_SITE_CACHE`, another omission found by running the
gate: toggling its generated concatenation tables must invalidate the cache.
