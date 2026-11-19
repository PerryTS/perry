// Compiled `#b` read, write and guard sites primed on a finished instance must
// still reject an instance of the same class whose `#b` is not defined yet
// (a field initializer that runs before it): the site's cached shape proves
// the slot, the slot's hole says the field is absent.
class Hole {
  static early = "";
  #a = Hole.early ? Hole.probe(this) : 0;
  #b = 7;
  static probe(h: Hole) {
    try {
      const v =
        Hole.early === "read" ? h.touch() : Hole.early === "write" ? h.write() : Hole.peek({ h });
      console.log(Hole.early, "no throw", v);
    } catch (e) {
      console.log(Hole.early, (e as Error).constructor.name, e instanceof TypeError);
    }
    return -1;
  }
  static peek(o: { h: Hole }) {
    return o.h.#b;
  }
  touch() {
    this.#b = this.#b + 1;
    return this.#b;
  }
  write() {
    this.#b = 9;
    return 9;
  }
  readA() {
    return this.#a;
  }
}

const done = new Hole();
for (let i = 0; i < 3; i++) console.log("primed", done.touch(), done.write(), Hole.peek({ h: done }));
for (const mode of ["read", "write", "peek"]) {
  Hole.early = mode;
  for (let i = 0; i < 3; i++) console.log("readA", new Hole().readA());
}
Hole.early = "";
const later = new Hole();
console.log(later.touch(), later.readA(), done.touch());
