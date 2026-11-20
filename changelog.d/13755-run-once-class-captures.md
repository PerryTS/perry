A class declared in a function that runs once and captures enclosing bindings
stays the class itself: its captures live in its single class environment, so
no per-evaluation class object is made. A sibling class method naming a later
class (`new Eff()` from `Base`) now constructs the same class the function
returns, with that class's prototype and `constructor`. A shorthand property
`{ C }` naming a class now reads the class its own scope declared; it used to
read the first class registered under that name when two functions each
declared a `class C`.
