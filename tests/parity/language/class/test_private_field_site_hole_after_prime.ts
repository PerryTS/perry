// A compiled `this.#b` site that has already read and written `#b` on a
// finished instance must still reject an instance of the same class whose
// `#b` is not defined yet (a field initializer that runs before it): the
// site's cached shape proves the slot, the slot's hole says the field is
// absent (#10501).
class Hole {
  static early = false;
  #a = Hole.early ? this.touch() : 0;
  #b = 7;
  touch() {
    this.#b = this.#b + 1;
    return this.#b;
  }
  readA() {
    return this.#a;
  }
}

const done = new Hole();
for (let i = 0; i < 3; i++) console.log("primed", done.touch());
Hole.early = true;
for (let i = 0; i < 3; i++) {
  try {
    new Hole();
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name, e instanceof TypeError);
  }
}
Hole.early = false;
const later = new Hole();
console.log(later.touch(), later.readA(), done.touch());
