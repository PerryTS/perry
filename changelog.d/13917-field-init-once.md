Fixed `new` of a subclass without its own constructor running the base class field initializers twice ("Cannot initialize #x twice") when the inherited constructor calls `super()`.
