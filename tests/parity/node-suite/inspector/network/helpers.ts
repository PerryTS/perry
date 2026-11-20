import inspector, { Network } from "node:inspector";

const methods = [
  "requestWillBeSent",
  "responseReceived",
  "loadingFinished",
  "loadingFailed",
  "dataSent",
  "dataReceived",
  "webSocketCreated",
  "webSocketClosed",
  "webSocketHandshakeResponseReceived",
] as const;

const payload = {
  requestId: "request-1",
  loaderId: "loader-1",
  timestamp: 1,
  wallTime: 1,
  type: "Document",
  frameId: "frame-1",
  request: {
    url: "https://example.test/resource",
    method: "GET",
    headers: {},
  },
  response: {
    url: "https://example.test/resource",
    status: 200,
    statusText: "OK",
    headers: {},
  },
  encodedDataLength: 12,
};

console.log("network type:", typeof inspector.Network);
console.log("named identity:", Network === inspector.Network);
console.log("keys:", Object.keys(inspector.Network).join(","));

for (const name of methods) {
  const fn = inspector.Network[name];
  const desc = Object.getOwnPropertyDescriptor(inspector.Network, name);
  const result = fn(payload as any);
  console.log(
    "method:",
    name,
    typeof fn,
    fn.length,
    fn.name,
    result === undefined,
    desc?.enumerable,
    desc?.writable,
    desc?.configurable,
  );
}

const session = new inspector.Session();
session.connect();
try {
  let genericCount = 0;
  let specificCount = 0;
  session.on("inspectorNotification", () => {
    genericCount++;
  });
  session.on("Network.requestWillBeSent", () => {
    specificCount++;
  });

  inspector.Network.requestWillBeSent(payload as any);
  await Promise.resolve();
  console.log("self session events:", genericCount, specificCount);
} finally {
  session.removeAllListeners();
  session.disconnect();
}
