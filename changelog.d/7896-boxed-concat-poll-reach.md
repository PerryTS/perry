Fixed the GC root-dominance moving-only audits so boxed string concatenation is
classified as poll-capable when it delegates non-string operands through
coercion and user code.
