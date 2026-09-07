# Next investigations

1. **Whole depth preflight in blocks.** The private depth24 probe classifies
   brackets along with quotes. It computes the inside-string mask by prefix
   XOR and applies a block's net depth only when no ordering can hit the depth
   limit or clamp at zero. Near those boundaries it visits brackets in order.
   Invalid backslashes outside strings trigger a scalar block fallback, so
   malformed-input behavior does not rely on JSON validity. Long quoted runs
   still use the existing bounded string helper. No allocation or GC calls.

   Four release tests pass, including exhaustive six-byte structural patterns
   across block boundaries and protected-end tests. Local, unqualified smoke
   measurements suggest about 54% less preflight time on record inputs, 41%
   on wide objects and 8% on escaped input versus depth23. These are isolated
   depth times. There is no runtime build or qualified remote result. Preserve
   the non-ARM runtime path when integrating; the standalone fallback is scalar.

2. **String-only key plans for stringify.** The current `Piece` also holds a
   32-byte numeric text buffer. Keys never need that variant, yet key plans
   and planner return values carry the larger representation. Prototype a
   three-word string plan with a nonzero quoted output length, UTF-16 length
   and escaped-source length. Zero source length can identify unescaped text:
   a string requiring an escape necessarily has nonzero source length.

   Keep the existing overflow, SSO, incomplete-tail and UTF-8/WTF-8 checks.
   Reuse the current escaped writer, root the parent object, and rederive key
   bytes after final allocation. Keep value plans unchanged initially. Inspect
   the actual return ABI and then measure complete stringify; a smaller plan
   does not by itself establish a CPU or RSS improvement. Earlier broader
   `Piece` changes were rejected, and their evidence remains in the repository.

Both investigations require full parse/stringify regression checks. Neither
needs a GC-policy change, a persistent pointer cache or per-object GC work.
