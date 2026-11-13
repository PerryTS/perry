`--target web`: a class that declares no constructor dropped its field
initializers (`class X { tag = "a" }` gave `new X().tag === undefined`), and a
derived class without one never ran its parent's constructor. Such a class now
gets the constructor TypeScript gives it: the parent's constructor runs with
the arguments, then the class's own field initializers.
