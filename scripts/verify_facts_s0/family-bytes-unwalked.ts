// family_reach: the callback is passed through a spread argument, a call
// edge the activation reach does not see (its body is not walked), so the
// solve holds no typed-array birth. The text rule names the constructor:
// the family is live without any help from the reach, and the family claim
// does not report (the reach's own "inactive body" claim does, until that
// call edge is modelled).
let n = 0;
[4].forEach(...[(x: number) => { n = new Uint8Array(x).length; }]);
console.log(n);
