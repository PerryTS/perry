Fixed class registration being keyed by class name: two classes with the same
name in one module each register their own methods, static methods, accessors
and constructors under their own class id.
