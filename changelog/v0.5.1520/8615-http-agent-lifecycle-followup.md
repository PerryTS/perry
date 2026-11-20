### Fixed

- Advanced `node:http` / `node:https` Agent and ClientRequest parity with
  `maxSockets` FIFO accounting, observable pool counts, `totalSocketCount`,
  `reusedSocket`, keep-alive connection options, a working default
  `createConnection`, both `http.globalAgent.addRequest` overloads, and
  correctly deferred legacy request `abort` events. Dynamic CommonJS
  `require("assert")` is also callable like Node, unblocking the upstream HTTP
  lifecycle tests (#4975).
