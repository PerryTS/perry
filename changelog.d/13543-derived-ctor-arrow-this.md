An arrow function created in a derived constructor before `super()` captured
the receiver handed to the base constructor, so when the base returned a
different object the arrow kept the stale `this`. Arrows created before the
binding are now registered and rebound to the object `super()` returns.
