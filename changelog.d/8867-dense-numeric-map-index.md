Numeric Maps now add a bounded range index when at least eight exact nonnegative integer keys form
a dense run. Sequential high-base IDs use a bounds check and array load instead of a hash-table
probe, while sparse, fractional, negative, tagged, and oversized ranges retain the existing hash
fallback.
