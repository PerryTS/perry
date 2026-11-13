`--target web`: a closure captured every variable by value, so its writes were
lost between calls and never reached the enclosing function or a sibling
closure. A variable a closure captures and someone mutates now lives in a shared
cell, as on native targets.
