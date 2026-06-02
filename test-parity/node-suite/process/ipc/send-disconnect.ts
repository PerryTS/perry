// Without an IPC channel, process.send and process.disconnect are undefined.
console.log("send:", process.send);
console.log("disconnect:", process.disconnect);
