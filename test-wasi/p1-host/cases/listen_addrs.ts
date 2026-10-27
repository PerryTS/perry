// Each listen() claims the next listener the host handed over, whatever port
// is asked for, and server.address() reports the address the host bound.
import * as net from "node:net";

const first = net.createServer(() => {});
const second = net.createServer(() => {});
const show = (name: string, server: net.Server) => {
  const address = server.address() as any;
  console.log(name, `${address.address}:${address.port}`);
};
first.listen(0, () => {
  show("first", first);
  second.listen(4000, () => {
    show("second", second);
    first.close();
    second.close();
  });
});
