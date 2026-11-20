`--target web`: a function declaration used as a value is one object with an ordinary `prototype` (`prototype.constructor` is the function), and `F.prototype = x` stores on it (#15075).
