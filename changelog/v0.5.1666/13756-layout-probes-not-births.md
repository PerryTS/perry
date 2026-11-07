Asking whether an object or a constructor prototype is on a runtime layout
(the node instance-layout probe, `EventEmitter.prototype` / `EventEmitterAsyncResource.prototype`
selection, the `ServerResponse` layout checks) no longer aborts in a program
whose realm does not reach that layout: a probe consults only reached layouts.
