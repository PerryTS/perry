A class with a dynamic parent (`extends` an expression) and its own constructor
no longer runs its field initializers twice.

The standalone-constructor lowering picked its pre-body field-staging mode from
`extends_name` alone, so an `extends_expr` class with an own constructor fell
through to mode `All` and staged its own fields before the body — after which
the body's `super()` lowering staged them again once the parent returned.

Three observable failures from that one cause: public initializers ran twice
(silent double side effects), private fields threw "Cannot initialize a private
field twice", and because the pre-body run happens before `super()` binds the
receiver, brands could land on the wrong object.

This is the `extends_name`-keying hazard the mixin pattern always exposes —
`function mixin(Base) { return class extends Base {...} }` has no literal
parent name to key on.
