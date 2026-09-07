### Fixed

- Resolve a named class expression's inner name inside `typeof`, including
  when its outer binding has a different name and the compiler registers the
  class under a generated key. The unresolved-global shortcut now excludes the
  active class inner name so lexical lowering can resolve the class binding;
  an HIR regression covers that path.
