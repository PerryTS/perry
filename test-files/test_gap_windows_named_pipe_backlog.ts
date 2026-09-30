import net from 'node:net';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const deadline = setTimeout(() => { console.log('pipe timeout'); process.exit(2); }, 10000);
  const payload = 'x'.repeat(4096);
  const responseChunk = 'y'.repeat(4096);
  const response = responseChunk.repeat(64);
  const server = net.createServer(socket => {
    let received = 0;
    socket.on('error', error => { throw error; });
    socket.on('data', data => {
      received += data.length;
      if (received === 262144) {
        console.log('request bytes:', received);
        for (let i = 0; i < 64; i++) socket.write(responseChunk);
        socket.end();
      }
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.on('error', reject);
    const pipe = '\\\\.\\pipe\\perry-backlog-end-' + process.pid;
    server.listen(pipe, () => {
      const client = net.connect(pipe);
      let text = '';
      client.on('error', reject);
      client.on('connect', () => {
        for (let i = 0; i < 64; i++) client.write(payload);
      });
      client.on('data', data => { text += data.toString(); });
      client.on('end', () => {
        console.log('complete response:', text === response);
        server.close(() => { clearTimeout(deadline); console.log('closed:', true); resolve(); });
      });
    });
  });
}
