// Stored sockets must use their payload family through dynamic method dispatch.
import net from 'node:net';
function send(endpoint: any, text: string) { return endpoint.write(text); }
function finish(endpoint: any) { endpoint.end(); }
const endpoints: any = {};
const server = net.createServer((socket: any) => {
  endpoints.accepted = socket;
  socket.on('data', (chunk: Buffer) => send(endpoints.accepted, 'echo:' + chunk.toString()));
});
server.listen(0, '127.0.0.1', () => {
  endpoints.client = new net.Socket();
  endpoints.client.on('connect', () => send(endpoints.client, 'stored'));
  endpoints.client.on('data', (chunk: Buffer) => {
    console.log(chunk.toString());
    console.log('endpoints', endpoints.client.remotePort === server.address().port,
      endpoints.accepted.remotePort === endpoints.client.localPort);
    finish(endpoints.client);
  });
  endpoints.client.on('close', () => server.close(() => console.log('closed')));
  endpoints.client.connect(server.address().port, '127.0.0.1');
});
