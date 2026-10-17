// Decision 150: a direct eval is no program-wide switch for method calls.
// Source built at run time and evaluated by a direct eval replaces a class
// method (the class is in its lexical scope) and a builtin prototype
// method. Every call site must see the replacement: the class prototype's
// and the builtin prototype's shadow bits are what the sites test.
class K {
  m() {
    return "orig";
  }
  viaThis() {
    return this.m();
  }
}

function callM(k) {
  return k.m();
}

function patch(code) {
  eval(code);
}

function sumCodes(s) {
  let t = 0;
  for (let i = 0; i < s.length; i++) t += s.charCodeAt(i);
  return t;
}

const k = new K();
const arr = [1, 2, 3];
console.log(callM(k), k.m(), new K().m(), k.viaThis(), sumCodes("abc"), arr.indexOf(2));
patch(["K.prototype.m = function () { return 'patched ' + (this instanceof K); };"].join(""));
console.log(callM(k), k.m(), new K().m(), k.viaThis());
patch(["String.prototype.charCodeAt = function () { return 1; };"].join(""));
console.log(sumCodes("abc"));
patch(["Array.prototype.indexOf = function () { return 'own indexOf'; };"].join(""));
console.log(arr.indexOf(2));
