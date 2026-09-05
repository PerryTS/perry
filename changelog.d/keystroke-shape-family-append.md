### Fixed

- Interning a shape descriptor no longer scans the keys array's whole
  descriptor history. `ShapeTableInner::family_push_back` / `facts_push_back`
  answer "is this id already here?" with a linear scan of the family, and a
  family accumulates every descriptor ever created for one keys array — so
  interning the *n*-th descriptor for a keys array cost O(n) and a render that
  keeps bumping a shape's semantic generation paid quadratic time. The two
  interning sites append ids that `alloc_shape_id` has just handed out, and
  that allocator never reuses a value, so the scan was provably dead work:
  they now use `IdList::append_unchecked`. On the compiled claude-code TUI
  `IdList::contains` was 6.2 % of main-thread leaf samples during a streamed
  reply and 5.9 % in the window after it, 95 % of it under
  `family_push_back`.
