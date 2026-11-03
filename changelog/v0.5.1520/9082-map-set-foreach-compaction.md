`Map.prototype.forEach` and `Set.prototype.forEach` now continue visiting live
entries when callback-side deletes cross the backing-store compaction threshold.
