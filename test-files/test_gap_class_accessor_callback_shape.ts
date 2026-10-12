let alias: any;
function replaceValue() { alias.data = '12'; return 0; }
class Changing {
  data: any = 3;
  get doubled() { const ignored = replaceValue(); return this.data * 2 + ignored; }
  set input(value: any) { this.data = value & 255; }
}
function read(c: Changing) { return c.doubled; }
function write(c: Changing, v: any) { c.input = v; }
const c = new Changing(); alias = c;
console.log('callback', read(c));
write(c, { valueOf() { alias.data = 'old'; return 9; } });
console.log('setter', c.data);
class Numeric {
  data = 3;
  get doubled() { return this.data * 2; }
}
function numeric(c: Numeric) { return c.doubled; }
const n = new Numeric();
console.log('numeric', numeric(n));
(n as any).data = '12';
console.log('widened', numeric(n));
