### Performance

- Compress validated multi-hop array-growth forwarding chains at their retained
  head. Generated indexed-access guards can now heal stale aliases after repeated
  capacity growth with their existing one-hop path instead of entering generic
  indexed lookup on every element access. On the full `codehz/ecs` suite, 11
  alternating Mac mini pairs reduced the 10k read-only query by 81.94% and the
  accumulation query by 76.59%, with 11/11 wins and every semantic oracle passing.
  Direct Node comparisons still leave 6.282x and 3.127x gaps respectively, so
  this change does not claim Node parity.
