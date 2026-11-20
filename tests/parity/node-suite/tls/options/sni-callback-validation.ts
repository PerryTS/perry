import tls from "node:tls";

function probe(label: string, value: any) {
  try {
    const server = tls.createServer({ SNICallback: value });
    console.log(label + ":", server instanceof tls.Server);
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
probe("function", (_name: string, callback: Function) => callback(null, null));
probe("null", null);
probe("number", 1);
probe("string", "callback");
