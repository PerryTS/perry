class Client {
  opts: any;
  static #made = 0;
  constructor(o: any) { this.#validate(o); this.opts = this.#init(o); Client.#made = Client.#count() + 1; }
  #validate(o: any) { if (o && o.bad) throw new TypeError("bad options"); }
  #init(o: any) { return { ...o, ok: true }; }
  static #count() { return Client.#made; }
  static made() { return Client.#count(); }
}
function attach(Base: any) { return class extends Base {}; }
const Sub = attach(Client);
console.log(new Sub({ a: 1 }).opts.ok, Client.made());
Object.setPrototypeOf({}, { x: 1 });
console.log(new Sub({ a: 2 }).opts.ok, Client.made());
console.log(new Client({}).opts.ok, Client.made());
