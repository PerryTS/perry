### Changed

- Region masked-window versioner (`#6794` follow-up): in the `ta_i32` fast copy,
  bind locals whose every in-region write is strictly-i32-bounded to a
  region-scoped i32 shadow slot, so a `>>>`/`&`/`^`/`| 0` bit-mixing chain on an
  untyped-init local (the bcryptjs `_encipher` shape, `l = lr[off]`) stays in
  native i32 instead of paying a ToInt32 tower per op. Removes the residual
  ToInt32 towers LLVM cannot fold on its own — ~11% on `bcryptjs.compareSync`
  with `Int32Array` S-boxes — with no change to the plain-array or slow copies.
