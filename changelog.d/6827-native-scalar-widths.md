## Added

- Complete the fixed-width scalar surface of `perry/native` with checked
  `i8`, `i16`, `u16`, and pointer-sized `isize` conversions and exact POD
  layouts. Invalid values throw instead of wrapping, truncating, or losing
  precision.
