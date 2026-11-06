// Decision 150: one direct eval the program never reaches must not slow or
// change builtin method calls elsewhere. sumCodes's charCodeAt is a direct
// builtin call (the String.prototype shadow bit is its only guard); the
// eval in neverCalled stays dead.
function neverCalled(code) {
  return eval(code);
}

function sumCodes(s) {
  let t = 0;
  for (let i = 0; i < s.length; i++) t = (t * 31 + s.charCodeAt(i)) >>> 0;
  return t;
}

let s = "";
for (let i = 0; i < 1000; i++) s += String.fromCharCode(32 + (i % 90));
let h = 0;
for (let r = 0; r < 2000; r++) h = (h * 7 + sumCodes(s) + r) >>> 0;
console.log(h, typeof neverCalled, s.charCodeAt(5), "abc".charCodeAt(1));
