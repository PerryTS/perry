An array literal held in a `const` that the program later mutates in place
(`push`, `pop`, `shift`, `unshift`, `splice`, `copyWithin`, `Set#add`) is no
longer scalarized, so `const s = [{ v: 1 }]; s.push(x)` compiles instead of
failing with `ArrayPush(1): local not in scope` (#12742).
