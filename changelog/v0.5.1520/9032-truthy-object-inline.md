### Changed

- `if (obj)`, `obj || fallback`, and `while (node)` on an object value no longer call the runtime truthiness predicate: the inline test now decides a POINTER-tagged value with a nonzero payload as truthy, on the branch edge that previously went straight to the call. Strings, BigInt, and the zero-payload case keep the call.
