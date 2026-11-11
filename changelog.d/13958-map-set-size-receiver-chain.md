A Map or Set `.size` read now follows the receiver's prototype chain. The intrinsic storage count is read only when the receiver has no own `size` and inherits the unwritten intrinsic `Map.prototype` / `Set.prototype`; a subclass getter, a relinked prototype or a redefined `size` accessor is honoured, matching node.
The check costs 13 instructions per `m.size` read (29 to 42).
