### Fixes
- Preserve closure initializers that earlier closures capture, fixing false temporal-dead-zone errors in mutually recursive `const` functions. Genuine TDZ errors now name the source binding, including captured reads and updates. Fixes #9721.
