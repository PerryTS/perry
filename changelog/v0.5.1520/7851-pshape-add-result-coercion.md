### Fixed

- Preserve JavaScript coercion when a declared-number `+` result feeds later arithmetic in typed-receiver `$pshape` fallbacks (#7506). Numeric locals now inherit declared-only proof from their initializer even when the HIR already typed them as `Number`, and compound operands are coerced before non-`+` arithmetic. Proven raw-f64 clones remain coercion-free.
