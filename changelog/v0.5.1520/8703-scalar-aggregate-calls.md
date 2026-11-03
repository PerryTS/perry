### Performance

- Scalar-replace short arrays of non-escaping object literals passed to known,
  bounded aggregate consumers. Their carrier arrays, descriptor objects,
  property/index accesses, and write barriers are now eliminated after
  conservative inlining and loop unrolling; identity-observing and otherwise
  escaping uses continue to materialize normally.
