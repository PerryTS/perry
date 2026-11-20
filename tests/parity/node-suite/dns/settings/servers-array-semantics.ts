import dns from "node:dns";

const sparse: string[] = [];
sparse[0] = "127.0.0.1";
sparse[2] = "0.0.0.0";
dns.setServers(sparse);
console.log("sparse:", dns.getServers().join("|"));

const accessed: number[] = [];
const shrinking = ["127.0.0.1", "192.168.1.1", "unused", "127.1.0.1"];
Object.defineProperty(shrinking, 2, {
  enumerable: true,
  get() {
    accessed.push(2);
    shrinking.length = 3;
    return "0.0.0.0";
  },
});
dns.setServers(shrinking);
console.log("shrinking:", dns.getServers().join("|"), accessed.join("|"));

const before = dns.getServers().join("|");
try {
  dns.setServers(["invalid"]);
} catch (error: any) {
  console.log("invalid:", error.name, error.code);
}
console.log("preserved:", before === dns.getServers().join("|"));
