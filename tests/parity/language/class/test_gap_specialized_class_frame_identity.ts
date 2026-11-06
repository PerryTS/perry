class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get item() { return this.value; }
  set item(value: T) { this.value = value; }
  read() { return this.item; }
}
const number = new Box<number>(1);
const text = new Box<string>("x");
number.item = 2;
text.item = "y";
console.log(number.read(), text.read(), number.value, text.value);
