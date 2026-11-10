function F() {}
const G: any = F.bind(null);
console.log(G.prototype === undefined);
try {
  class C extends G {}
  console.log("missing TypeError");
} catch (e) {
  console.log(e instanceof TypeError);
}
