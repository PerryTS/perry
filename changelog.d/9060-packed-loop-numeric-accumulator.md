### Changed

- Reduce loops — `for (let i = 0; i < arr.length; i++) s += arr[i]` — now run their fast versioned clone at full speed: the accumulator is tag-tested once in the preheader and proven a Number for the whole clone, so the per-element addition is a native `fadd` instead of a dynamic-`+` runtime call; and a proven-numeric accumulator's shadow-slot clear no longer disqualifies the clone from the call-free fast path. The isolated reduce loop went from 5.3× node to node parity.
