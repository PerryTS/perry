import { MessageChannel, receiveMessageOnPort } from 'node:worker_threads';

function check(label: string, clone: (value: any) => any) {
  const error: any = new Error('payload');
  const shared: any = { marker: 23, back: error };
  Object.defineProperty(error, 'cause', { value: shared, configurable: true });
  const result = clone({ error, shared });
  console.log(label, 'alias:', result.error.cause === result.shared,
    result.shared.back === result.error, result.error.cause.marker);

  const self: any = new Error('self');
  self.cause = self;
  const selfResult = clone(self);
  console.log(label, 'self:', selfResult.cause === selfResult);

  const missing: any = new Error('undefined');
  missing.cause = undefined;
  const undefinedResult = clone(missing);
  console.log(label, 'undefined:', Object.hasOwn(undefinedResult, 'cause'),
    undefinedResult.cause === undefined);

  let reads = 0;
  const accessor = new Error('accessor');
  Object.defineProperty(accessor, 'cause', {
    get() { reads++; throw new Error('must-not-read'); },
  });
  console.log(label, 'accessor:', Object.hasOwn(clone(accessor), 'cause'), reads);

  const reentrant: any = new Error('reentrant', { cause: 1 });
  Object.defineProperty(reentrant, 'stack', {
    get() { reentrant.cause = 23; return 'stack'; },
  });
  console.log(label, 'snapshot:', clone(reentrant).cause, reentrant.cause);

  const native = new Error('native', { cause: 23 });
  console.log(label, 'constructor:', clone(native).cause,
    Object.getOwnPropertyDescriptor(native, 'cause')!.enumerable);
}

check('clone', (value) => structuredClone(value));
const { port1, port2 } = new MessageChannel();
check('port', (value) => {
  port2.postMessage(value);
  return receiveMessageOnPort(port1)!.message;
});
port1.close();
port2.close();
