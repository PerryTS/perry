`--target web`: a class value's static fields are its own enumerable keys, so `Object.keys(C)`, `Object.entries(C)`, `"x" in C` and `hasOwnProperty` see them (#15021).
