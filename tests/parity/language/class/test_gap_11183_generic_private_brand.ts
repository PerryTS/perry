// Type arguments are erased: specializations share the source class's private
// names, while unrelated classes and objects with only its prototype do not.
class Box<T> {
  #value: T | undefined = undefined;
  #read() { return this.#value; }
  set(value: T) { this.#value = value; return this.#read(); }
  get value() { return this.#read(); }
  set value(value: T | undefined) { this.#value = value; }
  static has(object: any) { return #value in object; }
  static hasMethod(object: any) { return #read in object; }
}

function attempt(label: string, run: () => any): void {
  try { console.log(label, run()); }
  catch (error: any) { console.log(label, error.name); }
}

const plain = new Box();
const numberBox = new Box<number>();
const stringBox = new Box<string>();
attempt("plain", () => plain.set(1));
attempt("number", () => numberBox.set(2));
attempt("string", () => stringBox.set("three"));
console.log("brands", Box.has(plain), Box.has(numberBox), Box.has(stringBox));
console.log("method brands", Box.hasMethod(numberBox), Box.hasMethod(stringBox));
attempt("getter", () => numberBox.value);
attempt("setter", () => { numberBox.value = 4; return numberBox.value; });
attempt("borrow template", () => Box.prototype.set.call(numberBox, 5));
attempt("borrow specialization", () => numberBox.set.call(stringBox, 6));

class Other { #value = 0; }
const other = new Other();
const forged = Object.create(Box.prototype);
console.log("wrong brands", Box.has(other), Box.has(forged), Box.has({ "#value": 0 }));
attempt("reject other", () => numberBox.set.call(other, 7));
attempt("reject prototype only", () => numberBox.set.call(forged, 8));

class Child<T> extends Box<T> {
  #value = 9;
  own() { return this.#value; }
}
const child = new Child<number>();
attempt("derived base", () => child.set(10));
attempt("derived own", () => child.own());
console.log("derived brand", Box.has(child));

class MethodOnly<T> {
  #method() { return 11; }
  read() { return this.#method(); }
  static has(object: any) { return #method in object; }
}
const methodOnly = new MethodOnly<number>();
attempt("method only", () => methodOnly.read());
console.log("method only brand", MethodOnly.has(methodOnly));

class AccessorOnly<T> {
  get #value() { return 12; }
  read() { return this.#value; }
  static has(object: any) { return #value in object; }
}
const accessorOnly = new AccessorOnly<number>();
attempt("accessor only", () => accessorOnly.read());
console.log("accessor only brand", AccessorOnly.has(accessorOnly));

class Constructed<T> {
  #value: T;
  constructor(value: T) { this.#value = value; }
  read() { return this.#value; }
}
attempt("constructor", () => new Constructed<number>(13).read());

class ReturnObject {
  constructor(object: any) { return object; }
}
class Stamp<T> extends ReturnObject {
  #value = 14;
}
const stamped: any = {};
new Stamp<number>(stamped);
attempt("repeat initialization", () => new Stamp<string>(stamped));
