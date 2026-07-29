import { createServer, get } from "node:http";

const server = createServer((_req: any, res: any) => {
  res.statusCode = 200;
  res.setHeader("Content-Type", "application/grpc");
  res.setHeader("Trailer", "grpc-status, grpc-message");
  res.addTrailers({ "grpc-status": "0", "grpc-message": "ok" });
  res.end("payload");
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = get(
      {
        hostname: "127.0.0.1",
        port: address.port,
        path: "/",
        headers: { TE: "trailers" },
      },
      (res: any) => {
        let body = "";
        res.on("data", (chunk: any) => {
          body += String(chunk);
        });
        res.once("error", reject);
        res.on("end", () => {
          console.log("status:", res.statusCode);
          console.log("body:", body);
          console.log(
            "trailers:",
            JSON.stringify({
              "grpc-status": res.trailers["grpc-status"],
              "grpc-message": res.trailers["grpc-message"],
            }),
          );
          resolve();
        });
      },
    );
    req.once("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => {
    server.close(() => {
      console.log("closed");
      resolve();
    });
  });
}
