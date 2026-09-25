// turnloop — #11309: a standalone `ws` server and a `ws` client in ONE
// program: send one message, echo it back, close. Node exits; Perry hung.
//
// Two defects, each covered by a row below:
//
// 1. `wss.close()` issued while a client's closing handshake is still in
//    flight. Closing the listener dropped the host of every connection it had
//    already accepted, so the server never saw the client's close frame,
//    neither side finished the handshake, and the process never exited. Node's
//    contract is that in-flight connections finish. The first round-trip below
//    calls `ws.close()` and `wss.close()` in the same tick, exactly that
//    shape; a regression shows up as the harness timeout.
//
// 2. `'listening'` fired twice for a listener registered in the constructor's
//    tick (the bind is synchronous and queues the event, and `on()` queued a
//    replay on top). The count is printed, and the second round-trip opens its
//    client from inside `'listening'`, so a double fire would also open a
//    second client and print its rows twice.
//
// Nothing host-specific is printed, so the output is byte-comparable against
// `node --experimental-strip-types`.
import { WebSocket, WebSocketServer } from 'ws';

function closeInSameTick(): Promise<void> {
  return new Promise<void>((done) => {
    // The two sides' `'close'` events race each other, so they are recorded
    // and printed together once both have arrived.
    let serverCode: any = null;
    let clientCode: any = null;
    const settle = () => {
      if (serverCode === null || clientCode === null) return;
      console.log('A server close', serverCode);
      console.log('A client close', clientCode);
      done();
    };
    const wss: any = new WebSocketServer({ port: 0, host: '127.0.0.1' });
    wss.on('connection', (sock: any) => {
      sock.on('message', (d: any) => sock.send(String(d)));
      sock.on('close', (code: any) => {
        serverCode = code;
        settle();
      });
    });
    wss.on('listening', () => {
      const ws = new WebSocket('ws://127.0.0.1:' + wss.address().port);
      ws.on('open', () => ws.send('hello'));
      ws.on('message', (d: any) => {
        console.log('A client echo', String(d));
        ws.close();
        wss.close();
      });
      ws.on('close', (code: any) => {
        clientCode = code;
        settle();
      });
    });
  });
}

function serverInitiatedClose(): Promise<void> {
  return new Promise<void>((done) => {
    let listening = 0;
    const wss: any = new WebSocketServer({ port: 0, host: '127.0.0.1' });
    wss.on('connection', (sock: any) => {
      sock.on('message', (d: any) => {
        sock.send(String(d));
        sock.close(1000, 'done');
      });
    });
    wss.on('listening', () => {
      listening++;
      const ws = new WebSocket('ws://127.0.0.1:' + wss.address().port);
      ws.on('open', () => ws.send('again'));
      ws.on('message', (d: any) => console.log('B client echo', String(d)));
      ws.on('close', (code: any, reason: any) => {
        console.log('B client close', code, String(reason));
        wss.close(() => {
          console.log('B listening fired', listening);
          done();
        });
      });
    });
  });
}

async function main() {
  await closeInSameTick();
  await serverInitiatedClose();
  console.log('done');
}

main();
