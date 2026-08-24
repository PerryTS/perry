Fixed subclasses of runtime-valued ordinary functions failing during prototype
materialization when the parent function's lazy `.prototype` had not already
been read. `class Child extends Parent` now observes and links the same
prototype object as an ordinary `Parent.prototype` read.
