// A class expression bound to a function-local `var` captures that function's
// locals. A `new <Name>()` elsewhere, with no binding of its own, names the
// global `<Name>` — not the module's same-named nested class constructed
// without its environment.
function makeResponseModule() {
  const webidl = { util: { mark(_x: unknown) { return "marked"; } } };
  const prefix = "undici:";
  var Response = class {
    #body: string;
    mark: string;
    constructor(body: string) {
      this.mark = webidl.util.mark(this);
      this.#body = prefix + body;
    }
    get body(): string { return this.#body; }
  };
  const local = new Response("inside");
  console.log("inside factory:", local.body, local.mark);
  return { Response };
}

function install(mod: { Response: unknown }) {
  (globalThis as any).Response = mod.Response;
}

const mod = makeResponseModule();
console.log("before install:", typeof (globalThis as any).Response);
install(mod);

function serve(): string {
  // @ts-ignore -- the global was replaced by the class above
  const r = new Response("outside");
  return `${r.body} ${r.mark} ${r instanceof mod.Response}`;
}
console.log("after install:", serve());

class Plain {
  n: number;
  constructor(n: number) { this.n = n; }
}
console.log("module class:", new Plain(3).n);
