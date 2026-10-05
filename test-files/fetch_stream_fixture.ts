import { spawn } from 'node:child_process';
import { mkdtempSync, existsSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
export const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));

// An independent Node server is necessary to witness the actual wire close,
// rather than having client and server share Perry's stream implementation.
const tlsOptions = {"cert": "-----BEGIN CERTIFICATE-----\nMIIDJTCCAg2gAwIBAgIUZF3wbyk6BduDu+lEeegKd2ULMK8wDQYJKoZIhvcNAQEL\nBQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDUyNDE3NDI1NloXDTM2MDUy\nMTE3NDI1NlowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF\nAAOCAQ8AMIIBCgKCAQEAjekpyhiK0q4H8TQo01JTA564FZpOitgwvIYMe3qhf0dF\nlo2CbjxJcx5GOQ57k6vcNlLfIL2yV8f7hJNuFlfLAFvtm9pm45BvbsPvduW1AuSI\n3oA/fpfsQ5K1VgAPbLZFhdndCjoGW3/ZO8PbUC5DTge5luCfXoV0zFzZATbJxziy\nQZ9nYspc58Se6Xj0KhM3XCy2S7V5wVPRXo2nIW5ho83yHfKyVyKEew7nloxhrNAY\niRwHzVzBvCotdgZK/lBm1qsugHs31LR6T75izQGooIN1wz2V9kiHCW+s3CmgFCy2\n5AbN547xLwn4djh5Tz4lJhA4Rh0D0F/vzceL5ToJBwIDAQABo28wbTAdBgNVHQ4E\nFgQU1s+brNmcdkCqkncnW6rNlJpdiP0wHwYDVR0jBBgwFoAU1s+brNmcdkCqkncn\nW6rNlJpdiP0wDwYDVR0TAQH/BAUwAwEB/zAaBgNVHREEEzARgglsb2NhbGhvc3SH\nBH8AAAEwDQYJKoZIhvcNAQELBQADggEBAHFmvSxFCTHcqiocEHF3i0seBmNwWq40\nTtyVf9qyZYUZVqM/Z7tGDsNfNOhM+YscLs1ZTs8XzdpdYBEVyCLDYGjb4Cv6r5gS\nhr+E0NQBnPuker6Rw64nzahfWYjf/Eo+7nwUbCahTbXHAs43c4m0bmL02r1NxVmv\nBKGQKO/uR9Dy+3TKykNQkacKJ6oDxdTDovMUKlbwU/HlyzwK/HTm762cJfgZiMYM\nuru8x9wmqogCQSAz2q6a6q/CZfn1o7S5KiWd0FzinP+50g5cSL/ob0GJ8Jge1oI5\n5rap/3DFfnTn0zfJ60U52+BVFnOIqkYT7/g5N4laGrza73tYXq7FV4s=\n-----END CERTIFICATE-----", "key": "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCN6SnKGIrSrgfx\nNCjTUlMDnrgVmk6K2DC8hgx7eqF/R0WWjYJuPElzHkY5DnuTq9w2Ut8gvbJXx/uE\nk24WV8sAW+2b2mbjkG9uw+925bUC5IjegD9+l+xDkrVWAA9stkWF2d0KOgZbf9k7\nw9tQLkNOB7mW4J9ehXTMXNkBNsnHOLJBn2diylznxJ7pePQqEzdcLLZLtXnBU9Fe\njachbmGjzfId8rJXIoR7DueWjGGs0BiJHAfNXMG8Ki12Bkr+UGbWqy6AezfUtHpP\nvmLNAaigg3XDPZX2SIcJb6zcKaAULLbkBs3njvEvCfh2OHlPPiUmEDhGHQPQX+/N\nx4vlOgkHAgMBAAECggEACFfV8iDBQKOkqeSkJdBoOwVA01xQE8+kBeFnqHbMOdxp\n1fEZ4vs+Yjs8a6xTTZpEBxmWLqmYa5rBSckVJtEgiTPeY1RSyjw6oOt6D6Zvnuzq\nsxIdKYcrB8n/SUAVqBGLQtRNL4W7y/NXRTE9mpgtss+3dIxeMkNsW3t18qFS+Zhg\nTP8q984k+zl3QOz6sc5T39Unuk1g98LC2sjCXwKANzZRMBMigoGDnWgk9t86cEXM\nYWmyStS89HKEDmxWQMRIc/6zw5YC9Jo0cF2OJxGtN/O+LLeeNoJcdnlSAcyYRU1Q\nasJhtNkMwfMRrTVH0kQfF5X3a/aJfusiJnQBcvlVeQKBgQDF8rI8dlaQ7jNOfSoZ\nFhphZe1DriFaulRA9PUwrxEb/qvRstre0Egu967ILmqoqKfufyNT4W5JnWngliN6\nS7D9cvxpW0RsUQHZXMqZp7s6kt4hAdziuyC2Wx2y6+zFHkbOwJcaULYrSNHJCPOj\ncMu5TIplum+hnO9rMHKEpE0fAwKBgQC3h17rEy4uFbWPQD3fNjAi9QzIKX9wm8eD\nSYekgZaHpAjrLCa8oNR6qMxU5Cpn7I3o2HegSUe29jDAr8GMp47JYRTGMHUl1Zwa\nKtSGEH19sRhVUqIVW2h2/tysuaYpK1hFjPWM+KpKQFNzgt2EPf5057zE7gOHLcAL\nUccMgP1crQKBgQCy4h1SaHrYZHq3LoNRwli6thrRc9YuoH4taXD+uuaSTvZE/gWv\nH7hrwWcQ/mli229PJ1PspKc/HWMmE2giR669jCEwsMrHu/kYzjNE4oBfcYQNfhp4\nRzVLtlHDdFM226KPixnCLThDK35x14YdqHxiixnyzqW8/g6a5mBHIBeVswKBgQCT\ny79DndGdqTvqHbj1zWScci0V8F1BqSHVd1x1vSolF5NbF9YmJ3qVQOQ0JP6FbHmn\nntNPUFQhYkdGlQNQKwuQ3s5lAFcG3ev1IrK9OABnPTu0UnRWsKMC2SGLM4I9Ozu9\n3tNL8GDqpLzPk/6h5W7KZGifSnGq5cv3EaczSZk/jQKBgAcaLGi25ozeFgK1qvuQ\nWFTjLYV6KaMrGd5+NF+2a/NQsDGTZSF1egKUvE5QH5YNf37xWkqwvR3rsbenxLAG\naNYjvX+bUs4Mc/bgNkO51P9sH6YoKsuFzTTx4eR5ZS+dtfoiZMfzKkRBK4Baggrv\n7S9Q3thVBhvBcz19oFN2Rmvf\n-----END PRIVATE KEY-----"};
export async function fixture(secure = false) {
  const dir = mkdtempSync(join(tmpdir(), 'perry-fetch-stream-'));
  const ready = join(dir, 'ready');
  const code = `
    const http = require('node:http'), fs = require('node:fs'), z = require('node:zlib');
    const states = {};
    const create = ${JSON.stringify(secure)} ? handler => require('node:https').createServer(${JSON.stringify(tlsOptions)},handler) : handler => http.createServer(handler);
    const server = create((req, res) => {
      const path = req.url.split('?')[0];
      if (path === '/stats') { res.end(JSON.stringify(states)); return; }
      if (path === '/redirect') { res.writeHead(302, {location:'/drip'}); res.end(); return; }
      if (path === '/held-redirect') {
        const state = states[path] = {sent:0,closed:false,complete:false};
        res.writeHead(302,{location:'/drip','content-length':7}); res.flushHeaders();
        const timer=setTimeout(()=>{state.complete=true;state.sent=7;res.end('ignored');},1000);
        res.on('close',()=>{state.closed=true;clearTimeout(timer);});return;
      }
      if (path === '/gzip-bad' || path === '/gzip-short') {
        let bytes = z.gzipSync(Buffer.from('broken'));
        if (path === '/gzip-bad') bytes[bytes.length-8] ^= 1;
        else bytes = bytes.subarray(0,-4);
        res.writeHead(200,{'content-encoding':'gzip','content-length':bytes.length});res.flushHeaders();
        setTimeout(()=>res.end(bytes),200);return;
      }
      if (path === '/split-head') {
        const socket=res.socket, crlf=String.fromCharCode(13,10);
        socket.write('HTTP/1.1 200 OK'+crlf+'Content-Len');
        setTimeout(()=>socket.write('gth: 2'+crlf+crlf),80);
        setTimeout(()=>socket.end('OK'),240);return;
      }
      if (path === '/null') { res.writeHead(204); res.end(); return; }
      const state = states[path] = { sent:0, closed:false, complete:false };
      let timer;
      res.on('close', () => { state.closed = true; clearTimeout(timer); });
      if (path === '/rawgzip-large') {
        const data=Buffer.alloc(1048576); let seed=1;
        for(let i=0;i<data.length;i++) { seed=(Math.imul(seed,1664525)+1013904223)>>>0; data[i]=seed>>>24; }
        const bytes=z.gzipSync(data); let offset=0;
        res.writeHead(200,{'content-type':'application/octet-stream'}); res.flushHeaders();
        function sendLarge() {
          if(res.destroyed)return;
          if(offset===bytes.length){state.complete=true;res.end();return;}
          const end=Math.min(offset+65536,bytes.length);
          state.sent+=end-offset; const ready=res.write(bytes.subarray(offset,end));offset=end;
          if(ready)timer=setTimeout(sendLarge,40);else res.once('drain',()=>{timer=setTimeout(sendLarge,40);});
        }
        timer=setTimeout(sendLarge,200);return;
      }
      const coding = (path === '/gzip' || path === '/rawgzip') ? 'gzip' : path === '/br' ? 'br' : path === '/deflate' ? 'deflate' : '';
      const headers = {'content-type':'application/json'};
      if (coding && path !== '/rawgzip') headers['content-encoding'] = coding;
      if (path === '/form') headers['content-type']='application/x-www-form-urlencoded';
      if (path === '/error') headers['content-length'] = '99999';
      res.writeHead(200, headers); res.flushHeaders();
      if (req.method === 'HEAD') { res.end(); return; }
      let sink = res;
      if (coding) {
        sink = coding === 'gzip' ? z.createGzip() : coding === 'br' ? z.createBrotliCompress() : z.createDeflate();
        sink.pipe(res);
      }
      const label = new URL(req.url,'http://localhost').searchParams.get('label');
      const chunks = label ? ['{"value":',JSON.stringify(label)+',','"n":','42}'] : path === '/form' ? ['a=', 'stream', 'ing&', 'n=42'] : ['{"value":', '"stream', 'ing",', '"n":42}'];
      let index = 0;
      function send() {
        if (res.destroyed) return;
        if (path === '/large') {
          if (index++ >= Number(new URL('http://local'+req.url).searchParams.get('chunks') || 2048)) { state.complete=true; res.end(); return; }
          state.sent += 16384;
          if (res.write(Buffer.alloc(16384, 65))) timer=setTimeout(send,0);
          else res.once('drain',send);
          return;
        }
        if (path === '/error' && index === 1) { res.destroy(); return; }
        if (index === chunks.length) { state.complete=true; sink.end(); return; }
        const chunk = chunks[index++]; state.sent += Buffer.byteLength(chunk); sink.write(chunk);
        if (coding) sink.flush(coding === 'br' ? z.constants.BROTLI_OPERATION_FLUSH : z.constants.Z_SYNC_FLUSH);
        timer = setTimeout(send,80);
      }
      timer = setTimeout(send,200);
    });
    server.listen(0,'127.0.0.1',()=>fs.writeFileSync(process.argv[1],String(server.address().port)));
  `;
  const child = spawn('node', ['-e', code, ready], { stdio: 'ignore' });
  for (let i = 0; i < 200 && !existsSync(ready); i++) await delay(10);
  const url = (secure ? 'https://' : 'http://') + '127.0.0.1:' + readFileSync(ready, 'utf8');
  return { url, async close() { child.kill(); await delay(30); rmSync(dir, {recursive:true, force:true}); } };
}
