A public class field declared after a private one (in the same class or a
subclass) read the private field's value: the compiler placed private fields
after every public key, while construction claims each where it is declared
(`[parent, #ps, child]`). Codegen and the program facts now take field slots
from one rule (`perry_hir::private_names::construction_entries`). A
non-escaping `new` of a class with private elements is no longer
scalar-replaced: its private claims need the object.
