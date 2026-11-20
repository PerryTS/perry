Per-unit facts: a method of a published literal that hands its `this` to
code the solver cannot see (a value read from the global object or from an
unknown value, a parameter) now escapes the literal in the unit that owns
it, as whole-program facts do. The owner adds the literal to the method's
receiver because other units call the method on it, but it added it only to
the method's shared `this` node; a summarized body reads its receiver
through its summary instance, which that node does not feed, so the literal
never reached the call and kept its claims (`n` readonly and numeric while
the callee wrote a string). The receiver no call of the unit is behind now
goes to `Model::this_any`, which every instance of the body sees; a static
method of a published class takes the class object the same way.

Tests: perry-wpfacts two-unit tests (global callee, a value read from an
unknown value, a parameter callee, a class method, a static method; a
known own callee and a method not using `this` stay proven), each checking
per unit is never stronger than whole, and a linked program under both facts
modes with PERRY_VERIFY_FACTS=1 compared against node.
