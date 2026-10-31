Fix split-module native construction of indexed-owner loop regions. The
native IR reader accepted only the `!{!"NAME"}` metadata operand of
`llvm.read_register`, so a function whose typed-array owner region declares
its noalias scopes (`llvm.experimental.noalias.scope.decl`) failed with
"unsupported metadata operand" and the whole build refused to link
(opencode's `@aws-sdk/core` protocols module). The reader now builds any
inline metadata tuple of strings and tuples through the uniquing MDNode API,
on both the line and typed call paths, and attaches inline `!alias.scope` /
`!noalias` tuples on loads and stores, which it used to drop. Comma and
bracket splitting skips quoted strings, since scope domains embed function
names. A reader test holds the result equal to LLVM's own parse, and the
indexed-owner emitter test builds its live output through the reader.
