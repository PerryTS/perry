import { createServer, get } from "node:http";

const PORT = 19021;

const server = createServer((_req: any, res: any) => {
  res.statusCode = 200;
  res.setHeader("Content-Type", "application/grpc");
  res.setHeader("Trailer", "grpc-status, grpc-message");
  res.addTrailers({ "grpc-status": "0", "grpc-message": "ok" });
  res.end("payload");
});

server.listen(PORT, () => {
  get(
    { hostname: "127.0.0.1", port: PORT, path: "/", headers: { TE: "trailers" } },
    (res: any) => {
      let body = "";
      res.on("data", (chunk: any) => {
        body += String(chunk);
      });
      res.on("end", () => {
        console.log("status:", res.statusCode);
        console.log("body:", body);
        console.log("trailers:", JSON.stringify({ "grpc-status": res.trailers["grpc-status"], "grpc-message": res.trailers["grpc-message"] }));
        server.close(() => console.log("closed"));
      });
    }
  );
});

setTimeout(() => {}, 1500);
