class Client {
  opts: any;
  constructor(o: any) { this.opts = this.#init(o); }
  #init(o: any) { return { ...o, ok: true }; }
}
function attach(Base: any) { return class extends Base {}; }
const Sub = attach(Client);
console.log(new Sub({ a: 1 }).opts.ok);
console.log(new Client({}).opts.ok);
