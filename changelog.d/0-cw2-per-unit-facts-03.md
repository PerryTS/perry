### Changed

- Per-unit whole-program facts (`perry-wpfacts`, the default since #13132) carry phase 03 of the closed-world work: class instances' definite fields take store effects, TS enums and nested literals are contracts, a rest parameter takes only the extra arguments, a returned literal's field functions are contract functions. A closed class's constructor fed instances of its own class (trees, linked lists) keeps the class's layout proof across units, so `node.left`-style reads compile to direct slot loads again (js-compiled 23-binary-trees: -17.8% instructions).
