const { parentPort } = require("node:worker_threads");

const arrayBuffer = new ArrayBuffer(4);
new Uint8Array(arrayBuffer).set([1, 2, 3, 4]);
const sharedBuffer = new SharedArrayBuffer(4);
new Uint8Array(sharedBuffer).set([5, 6, 7, 8]);

parentPort.postMessage({ arrayBuffer, sharedBuffer }, [arrayBuffer]);
parentPort.postMessage({ detachedLength: arrayBuffer.byteLength });
