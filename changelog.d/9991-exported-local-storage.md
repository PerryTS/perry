### Fixed

- Register local variable exports after all module statements are lowered, independent of declaration order or initializer expression kind; remove the initializer whitelist.
- Match the exported module binding's LocalId so shadowing declarations inside try/catch/finally blocks cannot reclassify imported or function exports as local variables.
