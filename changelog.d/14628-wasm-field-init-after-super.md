`--target web`: a derived class ran its field initializers before `super()`, so
the parent constructor saw the child's fields. They now run when `super()`
returns.
