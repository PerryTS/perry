Split the full auto-optimize gap suite into twenty-four shards after nine
of twelve workers hit the existing 110-minute limit. Each previous slice is
partitioned into two without dropping fixtures or changing compilation,
snapshots, acceptance thresholds, fast-mode allocations or smoke workers.
