import * as http from "node:http";
const wrapBodyStream = Symbol("wrapBodyStream");
const server = http.createServer((req: any, res: any) => {
  let symOk = false;
  try {
    req[wrapBodyStream] = true;
    symOk = req[wrapBodyStream] === true;
  } catch (e: any) {
    res.statusCode = 500;
    res.end("SYMBOL_THREW:" + e?.message);
    return;
  }
  let body = "";
  req.on("data", (data: any) => { body += data; });
  req.on("end", () => {
    res.statusCode = 200;
    res.end("sym=" + symOk + ";body=" + body);
  });
});
server.listen(0, () => {
  const port = (server.address() as any).port;
  const data = "hello-post-body";
  const request = http.request(
    { host: "127.0.0.1", port, path: "/", method: "POST", headers: { "content-length": data.length } },
    (response: any) => {
      let out = "";
      response.setEncoding("utf8");
      response.on("data", (chunk: any) => { out += chunk; });
      response.on("end", () => {
        console.log("STATUS=" + response.statusCode);
        console.log("RESP=" + out);
        server.close();
      });
    },
  );
  request.write(data);
  request.end();
});
