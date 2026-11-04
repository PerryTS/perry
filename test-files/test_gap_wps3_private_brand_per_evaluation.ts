// WPS3 class C: a class expression evaluated twice is two classes: two private brands and two
// class objects, though the compiler has one template for both.
function make(tag: string) {
  return class {
    #v = 1;
    static tag = tag;
    static get(o: any) { return o.#v; }
    static has(o: any) { return #v in o; }
  };
}
const K1 = make("k1");
const K2 = make("k2");
function probe(K: any, o: any, label: string) {
  try { console.log(label, "read", K.get(o)); } catch (e: any) { console.log(label, e.constructor.name); }
  console.log(label, "in", K.has(o));
}
probe(K1, new K1(), "same");
probe(K1, new K2(), "cross");
console.log(K1.tag, K2.tag, K1 === K2, K1.get === K2.get);
