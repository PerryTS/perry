- **`Object.getPrototypeOf(CustomEvent)`.** A lazy Web constructor
  (`CustomEvent`, `MessageEvent`, `File`, `BroadcastChannel`) reports its
  parent constructor (`Event`, `Blob`, `EventTarget`) even before anything
  else touched the family; it read `Function.prototype` until then.
