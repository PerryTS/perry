import * as net from "node:net";
const variant = process.argv[2] || "cork"; const N = Number(process.argv[3] || "200");
const PARTS = ["P", "arse-message-", "B", "ind-portal-", "E"]; const MSG = PARTS.join("").length; // 5 writes, 30 bytes
const server = net.createServer((c) => {
  let got = 0;
  c.on("data", (d) => { got += d.length; while (got >= MSG) { got -= MSG; c.write("k"); } });
});
server.listen(0, "127.0.0.1", () => {
  const s = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
  s.setNoDelay(true);
  let i = 0; let t0 = 0;
  const batch = () => {
    if (variant === "cork") s.cork();
    if (variant === "concat") s.write(PARTS.join("")); else for (const p of PARTS) s.write(p);
    if (variant === "cork") s.uncork();
  };
  s.on("data", (d) => {
    i += d.length;
    if (i >= N) { console.log(`variant=${variant} checksum=${i} ms=${(performance.now() - t0).toFixed(1)}`); s.destroy(); server.close(); }
    else batch();
  });
  s.on("connect", () => { t0 = performance.now(); batch(); });
});