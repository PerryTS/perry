class Foo {
  #secret = 1;
  publicValue = 2;
  method() { return this.#secret; }
}
console.log("class instance:", new Foo());
console.dir(new Foo(), { showHidden: true });
