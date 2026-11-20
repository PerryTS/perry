### Fixed

- Iterator-helper objects now inherit a real `Iterator Helper.prototype.next`
  method. Reading `helper.next` therefore returns a callable with the same
  receiver and brand-check behavior as Node, so patterns such as
  `const original = helper.next.bind(helper)` can delegate to the builtin
  advance instead of failing because the property read returned `undefined`
  (#9068).
