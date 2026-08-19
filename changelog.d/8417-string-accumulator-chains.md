### Performance

- Keep the growing prefix of loop-local string accumulator chains on Perry's
  amortized append path while fusing only the fixed-size suffix. The reported
  16,000-iteration forced-read benchmark drops from 124 ms to 1 ms and no
  longer exhibits quadratic growth.
