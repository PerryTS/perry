// Closed World: a definitely-assigned array that no code but its own element
// reads names is CONFINED, so its element reads skip the descriptor query.
// An array that escapes is not confined; a confined one still honours an
// accessor that an Array.prototype getter installs through `this`.

// Escapes into Object.defineProperty: not confined.
var ESC: any = (function () {
  var a: any = [];
  a.push(1);
  a.push(2);
  return a;
})();
function readEsc(i: number): any {
  return ESC[i];
}
Object.defineProperty(ESC, 0, { get() { return "accessor"; } });
console.log(readEsc(0), readEsc(1), String(readEsc(2)));

// Confined: only element and length reads name it.
var CON: any = (function () {
  var a: any = [];
  for (var i = 0; i < 4; i++) a[a.length] = "c" + i;
  return a;
})();
function readCon(i: number): any {
  return CON[i];
}
function lenCon(): number {
  return CON.length;
}
console.log(readCon(0), readCon(3), String(readCon(4)), lenCon());

// A prototype getter reached by an out-of-bounds read receives CON as `this`
// and installs an own accessor on it; later reads must see the accessor.
Object.defineProperty(Array.prototype, "9", {
  configurable: true,
  get() {
    Object.defineProperty(this, 1, { get() { return "installed"; }, configurable: true });
    return "proto";
  },
});
console.log(readCon(9), readCon(1), readCon(2));
delete (Array.prototype as any)[9];
console.log(readCon(1), String(readCon(9)), lenCon());
