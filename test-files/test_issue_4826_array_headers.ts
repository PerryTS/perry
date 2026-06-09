import { createServer } from "node:http";

const s = createServer((req, res) => {
  if (req.url === "/wh") {
    // writeHead bulk-header array path
    res.writeHead(200, { "set-cookie": ["a=1; Path=/", "b=2; Path=/"] });
    res.end("ok");
  } else {
    // setHeader array path + scalar header
    res.setHeader("set-cookie", ["c=3; Path=/", "d=4; Path=/"]);
    res.setHeader("content-type", "text/plain");
    res.writeHead(200);
    res.end("ok");
  }
});

s.listen(3456, () => {
  console.log("listening");
});
