`bcrypt` `hash`, `hashSync`, `genSalt` and `genSaltSync` derive the cost from out-of-range rounds as node `bcrypt` does: `Infinity` and `-Infinity` give cost 4, and `-5` gives 31 (#15213).
