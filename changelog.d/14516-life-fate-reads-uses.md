### Changed
- The parameter fate (compiler-internal, read only by the life census) reads uses that keep nothing: bodies with default and rest parameters are walked, `typeof` and truthiness tests drop, conversions drop a binding proven to hold only Numbers, reassigning a parameter keeps nothing, a never-written copy is the parameter, and a literal shape's constructor keeps its arguments by receiver.

### Fixed
- An argument past a function's declared parameters was answered Dropped even when a rest parameter or `arguments` collects it; it now takes their fate (Kept).
- The life census wrote every module's parameter rows twice (the births pre-pass registered it too).
