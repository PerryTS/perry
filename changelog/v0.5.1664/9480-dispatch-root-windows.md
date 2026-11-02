### Performance

- Narrow instance-method dispatch temporary roots to each operand's actual
  collection suffix. Ordinary calls no longer root the final heap argument or
  operands followed only by the audited own-field/class-id probes, while
  rest/synthetic argument construction and other post-argument work retain
  their conservative protection. This removes the root-slot overhead added by
  the #9479 soundness fix without weakening its moving-GC guarantee. (#9480)
