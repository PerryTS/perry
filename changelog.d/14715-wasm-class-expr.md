On the web target a class declared inside a function body is a constructor:
`function e() { class B { who() { return "base"; } } return new B().who(); }`
returns "base" instead of failing on an undefined class.
