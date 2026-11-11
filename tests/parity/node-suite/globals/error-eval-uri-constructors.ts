function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

function showError(label: string, error: any, ctor: any) {
  show(label + " ctor name", ctor.name);
  show(label + " name", error.name);
  show(label + " message", error.message);
  show(label + " self dynamic", error instanceof ctor);
  show(label + " error base", error instanceof Error);
}

const evalDirect: any = new EvalError("msg");
showError("eval direct", evalDirect, EvalError);
show("eval direct self static", evalDirect instanceof EvalError);

const uriDirect: any = new URIError("msg");
showError("uri direct", uriDirect, URIError);
show("uri direct self static", uriDirect instanceof URIError);

const evalGlobal: any = new globalThis.EvalError("global");
showError("eval global", evalGlobal, globalThis.EvalError);
show("eval global self static", evalGlobal instanceof globalThis.EvalError);

const uriEmpty: any = new URIError();
show("uri empty name", uriEmpty.name);
show("uri empty message empty", uriEmpty.message === "");
show("uri empty self static", uriEmpty instanceof URIError);

const ReboundEval: any = EvalError;
const evalRebound: any = new ReboundEval("rebound");
showError("eval rebound", evalRebound, ReboundEval);
