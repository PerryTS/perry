// Methods a `class … extends Promise` declares must be callable through a
// computed key and through the dynamic dispatch tower, not only by name.
class Api extends Promise<any> {
  rp: Promise<any>;
  constructor(rp: Promise<any>) {
    super((z: any) => {
      z(null);
    });
    this.rp = rp;
  }
  async withResponse() {
    const [d, r] = await Promise.all([this.parse(), this.asResponse()]);
    return { data: d, response: r };
  }
  asResponse() {
    return this.rp.then((q: any) => q.response);
  }
  parse() {
    return this.rp.then((q: any) => q.data);
  }
  sync() {
    return 4;
  }
  then(a?: any, b?: any): any {
    return this.parse().then(a, b);
  }
  catch(a?: any): any {
    return this.parse().catch(a);
  }
}

const api: any = new Api(Promise.resolve({ response: "R", data: "D" }));
function call(k: string) {
  return api[k]();
}
console.log(call("sync"));
const p = call("withResponse");
console.log(p instanceof Promise, typeof p.catch);

async function main() {
  console.log(await p);
  const r = await api.withResponse().catch((e: any) => {
    throw e;
  });
  console.log(r);
  const bad: any = new Api(Promise.reject(new Error("boom")));
  try {
    await bad.withResponse().catch((e: any) => {
      throw e;
    });
  } catch (e: any) {
    console.log("caught", e.message);
  }
}
main();
