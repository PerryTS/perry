Split the full auto-optimize gap suite into twenty-four shards after nine
of twelve workers hit the existing 110-minute limit. Split the fast PR
arm into twelve shards after its six-way shard 1 also exhausted that bound.
Each previous slice is partitioned into two without dropping fixtures or
changing compilation, snapshots, acceptance thresholds or smoke workers.
