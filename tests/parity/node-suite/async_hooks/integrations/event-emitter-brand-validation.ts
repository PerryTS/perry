import { EventEmitterAsyncResource } from "node:events";

const prototype = EventEmitterAsyncResource.prototype;
console.log(
  "event resource prototype surface:",
  typeof prototype.emit,
  typeof prototype.emitDestroy,
  "asyncId" in prototype,
  "triggerAsyncId" in prototype,
  "asyncResource" in prototype,
);

function probe(label: string, operation: () => unknown) {
  try {
    operation();
    console.log(label, "no-throw");
  } catch (error: any) {
    console.log(
      label,
      error.name,
      /private member/.test(String(error.message)),
    );
  }
}

probe("event resource emit brand:", () => prototype.emit());
probe("event resource destroy brand:", () => prototype.emitDestroy());
probe("event resource asyncId brand:", () => prototype.asyncId);
probe("event resource triggerAsyncId brand:", () => prototype.triggerAsyncId);
probe("event resource asyncResource brand:", () => prototype.asyncResource);
