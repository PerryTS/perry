import { BlockList, SocketAddress } from "node:net";

function run(label: string, action: () => void) {
  try {
    action();
  } catch (error: any) {
    console.log(label, error.code);
  }
}

run("saved-address", () => {
  const list = new BlockList();
  const add = list.addAddress;
  const address = new SocketAddress({ address: "::1", family: "ipv6" });
  const result = add.call(list, address, 42 as any);
  console.log("saved-address", result === undefined, list.check("::1", "ipv6"));
});

run("saved-range", () => {
  const list = new BlockList();
  const add = list.addRange;
  const first = new SocketAddress({ address: "127.0.0.2" });
  const last = new SocketAddress({ address: "127.0.0.4" });
  const result = add.call(list, first, last, 42 as any);
  console.log("saved-range", result === undefined, list.check("127.0.0.3"));
});

run("saved-subnet", () => {
  const list = new BlockList();
  const add = list.addSubnet;
  const network = new SocketAddress({ address: "127.0.1.0" });
  const result = add.call(list, network, 24, 42 as any);
  console.log("saved-subnet", result === undefined, list.check("127.0.1.9"));
});

run("mixed-start", () => {
  const list = new BlockList();
  const first = new SocketAddress({ address: "127.0.0.2" });
  const result = list.addRange(first, "127.0.0.4", "ipv4");
  console.log("mixed-start", result === undefined, list.check("127.0.0.3"));
});

run("mixed-end", () => {
  const list = new BlockList();
  const last = new SocketAddress({ address: "127.0.0.4" });
  const result = list.addRange("127.0.0.2", last, "ipv4");
  console.log("mixed-end", result === undefined, list.check("127.0.0.3"));
});
