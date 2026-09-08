**Dynamic-heritage classes now retain the receiver created by `super()` when
their runtime superclass resolves to `Object`.**

Perry replayed these constructors against a provisional instance but discarded
the constructor's effective return value. Writes after `super()` therefore
landed on the new receiver while `new` returned the abandoned one. Dynamic
construction now propagates the replacement receiver and preserves its derived
prototype and per-evaluation private brand, matching Node for both shared class
references and fresh captured class expressions.
