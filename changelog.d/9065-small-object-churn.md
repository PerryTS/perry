Improved repeated add/delete churn on small objects. A first delete now forks
an owned tombstoned key layout instead of compacting a transition-cache-shared
layout back to empty, allowing subsequent stable-token delete and append paths
to keep their inline caches alive.

This removes the per-iteration keys-array allocation loop while preserving the
existing bounded append/squeeze behavior and both flag states.
