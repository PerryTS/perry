const req = new Request("http://example.test/path?q=1", { method: "POST" });
const anyReq: any = req;

function readViaParam(request: any) {
  console.log("param typeof url:", typeof request.url);
  console.log("param url:", request.url);
}

console.log("direct typeof url:", typeof req.url);
console.log("direct url:", req.url);
console.log("any typeof url:", typeof anyReq.url);
console.log("any url:", anyReq.url);
readViaParam(req);
