### Performance

- Keep string accumulators stored in async/closure variable boxes, captures,
  and module globals on Perry's amortized append path. Ordinary reads and
  local-to-local assignments still demote extracted aliases before later
  in-place growth.
