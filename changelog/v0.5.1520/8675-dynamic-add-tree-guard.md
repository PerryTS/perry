### Performance

- Lower dynamic `+` trees with three or more leaves behind one shared numeric
  guard. Number-only executions use native additions, while the cold arm keeps
  the original tree, evaluation order, associativity, and complete dynamic
  string/BigInt/Symbol/object-coercion behavior. On the `codehz/ecs` 10k
  accumulation query, 11 alternating Mac mini pairs reduced median time by
  4.11%, with 11/11 wins and every semantic oracle passing. The read-only
  query was neutral, and this change does not claim Node parity.
