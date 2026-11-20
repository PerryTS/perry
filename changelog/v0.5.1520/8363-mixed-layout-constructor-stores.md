### Performance

- Skip prologue-only constructors for mixed pointer/raw-f64 layouts, including
  typed `numberParam + finiteLiteral` field initialization, while retaining
  pointer write barriers. On issue #8289's `tree_wide` fixture this reduces
  retired instructions by 67.4% (23.41B to 7.62B) with unchanged output.
