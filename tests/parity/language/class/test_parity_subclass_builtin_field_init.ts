class A extends Error {
  pubv = 42;
  #priv = 7;
  arr = [1, 2, 3];
  readPub() { return this.pubv; }
  readPriv() { return this.#priv; }
  readArr() { return this.arr; }
}
const a = new A();
if (a.readPub() !== 42) throw new Error("pubv: " + a.readPub());
if (a.readPriv() !== 7) throw new Error("priv: " + a.readPriv());
if (!a.readArr().includes(2)) throw new Error("arr: " + JSON.stringify(a.readArr()));
console.log("OK");
