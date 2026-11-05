A closure created before a function-nested class declaration kept reading the
wrong class in three shapes the forward class binding did not cover. Inside an
async function or generator, a loop body's forward binding was snapshotted into
a per-iteration cell before the declaration and the declaration then filled a
different cell, so the closure threw a TypeError. Every class declaration that
an earlier closure of its block reads now gets the same pre-registered binding,
whether or not it looks like it is evaluated more than once or captures
anything (a prediction of that missed a closure made in an earlier class
declaration's computed key and a subclass of a class that is fresh only because
it captures a parameter), so a later evaluation's closures resolve their own
class instead of the latest one; a class that stays the shared template
initializes the binding with the template. The pre-registration also runs for
a block directly in module code, such as a loop body. The generator transform creates
such a binding's cell where its box was preallocated and the declaration writes
into it, also when the declaration sits in the try block that a `using`
declaration generates around the rest of its block.
The dead zone of such a binding is kept: the generator transform's per-iteration
cell for a binding whose box was allocated in the dead zone is
`[value, initialized]` and a read before the declaration throws the
ReferenceError instead of returning `undefined`; the first-evaluation guard
that picks a shared template's static form reads an uninitialized class as
`undefined` instead of throwing. An object method's parameter defaults count as
reads by an earlier closure.
The generator transform reads that cell without the dead-zone check when it snapshots a class's captured bindings, so a class that names a later class in a loop of an async function or generator no longer throws. An assignment, update or compound assignment to the binding before its declaration throws the ReferenceError and leaves the cell uninitialized (the declaration's own store marks it initialized). A class named only in a binding pattern's default or computed key, a for-in/of head, a catch parameter or an array assignment pattern inside a closure is captured as a forward read too.
The scan for closure reads of a later binding is a swc `Visit` that walks every expression position (computed keys of object and class members, class property initializers, decorators, `super` arguments, tagged templates, optional chains, `using` declarations) and tracks only the closure boundary and the shadowing positions, so a syntax kind cannot be skipped. A class declaration or expression that names a later class declaration of the same function body, loop body or block, with an earlier closure reading that class, keeps reading it through the class's static name (`ClassRef`, which is the class itself for its first evaluation), as it did before the binding was pre-registered: the class does not capture the not yet initialized binding, so there is no snapshot to repair afterwards (a capture array whose other bindings left scope with their block, several evaluations of one declaration, an instance created before the later class initialized). Only the member bodies that run later (methods, accessors, constructors, static blocks, instance field initializers) are kept off the pending binding, and the scan that registers the binding leaves out the same bodies and nothing else: a closure made while the class is defined (a computed key, the `extends` expression, a static field initializer) reads the binding's box live, like any other closure, and so does a closure that outlives the class declaration. The function that declares the binding owns it (a `catch` clause is part of its function), bindings are picked by local id, and a pending binding of the same name in an enclosing scope or function is kept off together with the inner one, so a class in a `catch` clause or under an inner block's class of the same name reads its own later class. A class built in a nested function still captures the binding. A generator's parameter defaults and patterns count when deciding whether it needs the closure path, so a default naming a later class captures it.
A direct `eval` reads every later class declaration of its scope, because the source string can name any of them and can create a closure that outlives the call: `const read = () => eval("C"); class C extends Error {}` returns the declared class, and so does `const read = eval("() => C"); class C extends Error {}` (a constant string is lowered into a real read of the class, which a scan of the closure's own identifiers does not see). The eval counts in a closure and in the function body itself, and a class declared before it is not affected. A static or instance method that calls `eval` while a later class is pending is lowered with the hidden binding left out of the bindings object that `eval` receives, so the hidden entry's name is never spelled into the generated source.
