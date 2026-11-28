- **`Function.prototype.caller` / `.arguments` accessors.** Each half is its own
  function (`get caller`, `set caller`, `get arguments`, `set arguments`), as in
  node, instead of `%ThrowTypeError%` for both; reading or writing through them
  still throws the restricted-access TypeError.
