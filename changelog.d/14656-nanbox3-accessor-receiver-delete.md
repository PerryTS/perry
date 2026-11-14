`super[key] = value` keeps a primitive Receiver. `normalize_accessor_receiver`
turned a Receiver with clear top 16 bits (a denormal Number) and an integral
Number above 65536 into a heap pointer at those bits before the inherited
setter ran, so a method called with a Number `this` handed its setter a bogus
object. The function is deleted: the setter's `this` is the Number, and a write
with no setter fails with a TypeError in strict code, as in Node.
