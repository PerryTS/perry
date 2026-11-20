// The client res.trailers keys are lowercase, as in Node, whatever the wire spelling.
import * as http from "node:http";
import * as net from "node:net";

const srv = net.createServer((s) => {
  s.once("data", () => {
    s.write("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\npayload\r\n0\r\nX-End: yes\r\nX-Sum: AbC\r\n\r\n");
    s.end();
  });
});
srv.listen(0, "127.0.0.1", () => {
  const port = (srv.address() as net.AddressInfo).port;
  const req = http.request({ host: "127.0.0.1", port, path: "/", headers: { TE: "trailers" } }, (res) => {
    let body = "";
    res.setEncoding("utf8");
    res.on("data", (d) => (body += d));
    res.on("end", () => {
      console.log("body", body);
      console.log("keys", Object.keys(res.trailers).sort().join(","));
      console.log("x-end", res.trailers["x-end"], "x-sum", res.trailers["x-sum"]);
      srv.close();
    });
  });
  req.end();
});
