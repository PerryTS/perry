import { BlockList, SocketAddress } from "node:net";

const list = new BlockList();
const first = new SocketAddress({ address: "127.0.0.1" });
const start = new SocketAddress({ address: "127.0.0.2" });
const end = new SocketAddress({ address: "127.0.0.4" });

list.addAddress(first);
list.addRange(start, end);
list.addSubnet(new SocketAddress({ address: "127.0.1.0" }), 24);

console.log(
  list.check(first),
  list.check(new SocketAddress({ address: "127.0.0.3" })),
);
console.log(list.check(new SocketAddress({ address: "127.0.1.255" })));
console.log(JSON.stringify(list.rules));
