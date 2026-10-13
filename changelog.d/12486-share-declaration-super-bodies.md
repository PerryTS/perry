Class methods using `super` share one implementation across direct calls and
the function object's callable entry. Once-evaluated declarations use the
direct body; fresh templates use one explicit-owner body with a small direct
adapter. The shared home operation accepts the lexical class or selected
callable, preserving each evaluation's captured home. Generator factories
retain their existing owner ABI. This removes the second full method body
introduced by classsem; birth-shape and name setup are unchanged.
