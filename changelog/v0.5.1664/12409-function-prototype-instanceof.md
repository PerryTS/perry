`instanceof Object` now follows a callable value's live prototype chain, so changing `Function.prototype` to inherit from `null` is reflected for functions and arrows.
