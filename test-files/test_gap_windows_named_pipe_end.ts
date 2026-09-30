import net from 'node:net';

if (process.platform !== 'win32') {
  console.log('Windows only');
} else {
  const deadline = setTimeout(() => { console.log('pipe timeout'); process.exit(2); }, 10000);
  const server = net.createServer(socket => {
    socket.on('error', error => { throw error; });
    socket.on('data', data => socket.end(data));
  });
  await new Promise<void>((resolve, reject) => {
    server.on('error', reject);
    server.listen('\\\\.\\pipe\\perry-end-regression-' + process.pid, () => {
      const client = net.connect('\\\\.\\pipe\\perry-end-regression-' + process.pid);
      let text = '';
      client.on('error', reject);
      client.on('connect', () => client.write('pipe payload'));
      client.on('data', data => { text += data.toString(); });
      client.on('end', () => {
        console.log('echo:', text === 'pipe payload');
        console.log('EOF:', true);
        server.close(() => { clearTimeout(deadline); console.log('closed:', true); resolve(); });
      });
    });
  });
}
