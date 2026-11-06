import * as net from "node:net";

const samples = [
  "::1",
  "::ffff:127.0.0.1",
  "fe80::1%lo0",
  "fe80::1%eth0",
  "fe80::1%18",
  "::1%lo0",
  "fe80::1%",
  "fe80::1%bad_zone",
  "[::1]",
  "127.0.0.1%lo0",
];

for (const value of samples) {
  console.log(
    "ip",
    JSON.stringify(value),
    net.isIP(value),
    net.isIPv4(value),
    net.isIPv6(value),
  );
}
