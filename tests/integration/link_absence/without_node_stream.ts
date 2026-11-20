// Array subclass and ordinary object paths must not install node:stream.
class Numbers extends Array<number> {
  constructor() { super(3); this.fill(7); }
}
const values = new Numbers();
console.log(values.length, values[0], values[2]);
const object: any = {a: 1};
object.b = 2;
console.log(object.a, object.b, object instanceof Object);
