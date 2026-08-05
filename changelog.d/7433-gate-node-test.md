size: `node:test` is now compiled only into programs that import it — the runner was the sole retainer of the JSON serializer, so hello world drops 82,672 bytes (4,791,416 → 4,708,744).
