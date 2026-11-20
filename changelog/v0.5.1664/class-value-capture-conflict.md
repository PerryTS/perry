**runtime: a class function object keeps its evaluation state and its prototype link in separate captures (fixes the main build)**

#11840 and #11843 both claimed capture 1 of the class function object (the
evaluation state and the prototype link); their merge left conflict markers in
`class_value.rs`. The evaluation state keeps capture 1 (codegen writes it), the
prototype link moves to capture 2, and the object is minted with three captures.
