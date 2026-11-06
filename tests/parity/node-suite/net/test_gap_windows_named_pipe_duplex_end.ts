import net from 'node:net';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const deadline = setTimeout(() => { console.log('pipe timeout'); process.exit(2); }, 10000);
  const server = net.createServer(socket => {
    socket.on('error', error => { throw error; });
    socket.on('data', () => socket.end('response after end'));
  });
  await new Promise<void>((resolve, reject) => {
    server.on('error', reject);
    const pipe = '\\\\.\\pipe\\perry-duplex-end-' + process.pid;
    server.listen(pipe, () => {
      const client = net.connect(pipe);
      let text = '';
      client.on('error', reject);
      client.on('connect', () => client.end('request'));
      client.on('data', data => { text += data.toString(); });
      client.on('end', () => {
        console.log('response after end:', text === 'response after end');
        server.close(() => { clearTimeout(deadline); console.log('closed:', true); resolve(); });
      });
    });
  });
}
