// `new MessageChannel()` is the only asynchronous construct of this program.
// Its message is delivered by a later task, so `main` must reach the event
// loop: a constructor named by `new` is a global read like any other, and
// only the known synchronous ones leave the "schedules nothing" proof intact.
const channel = new MessageChannel();
channel.port1.onmessage = (event: any) => {
  console.log("message", event.data);
  channel.port1.close();
};
channel.port2.postMessage(7);
console.log("sync end");
