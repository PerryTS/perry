// A Node child is the independent wire reader for both Node and native servers.
import * as http from 'node:http';
import * as https from 'node:https';
import { spawn } from 'node:child_process';

const key = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCN6SnKGIrSrgfx
NCjTUlMDnrgVmk6K2DC8hgx7eqF/R0WWjYJuPElzHkY5DnuTq9w2Ut8gvbJXx/uE
k24WV8sAW+2b2mbjkG9uw+925bUC5IjegD9+l+xDkrVWAA9stkWF2d0KOgZbf9k7
w9tQLkNOB7mW4J9ehXTMXNkBNsnHOLJBn2diylznxJ7pePQqEzdcLLZLtXnBU9Fe
jachbmGjzfId8rJXIoR7DueWjGGs0BiJHAfNXMG8Ki12Bkr+UGbWqy6AezfUtHpP
vmLNAaigg3XDPZX2SIcJb6zcKaAULLbkBs3njvEvCfh2OHlPPiUmEDhGHQPQX+/N
x4vlOgkHAgMBAAECggEACFfV8iDBQKOkqeSkJdBoOwVA01xQE8+kBeFnqHbMOdxp
1fEZ4vs+Yjs8a6xTTZpEBxmWLqmYa5rBSckVJtEgiTPeY1RSyjw6oOt6D6Zvnuzq
sxIdKYcrB8n/SUAVqBGLQtRNL4W7y/NXRTE9mpgtss+3dIxeMkNsW3t18qFS+Zhg
TP8q984k+zl3QOz6sc5T39Unuk1g98LC2sjCXwKANzZRMBMigoGDnWgk9t86cEXM
YWmyStS89HKEDmxWQMRIc/6zw5YC9Jo0cF2OJxGtN/O+LLeeNoJcdnlSAcyYRU1Q
asJhtNkMwfMRrTVH0kQfF5X3a/aJfusiJnQBcvlVeQKBgQDF8rI8dlaQ7jNOfSoZ
FhphZe1DriFaulRA9PUwrxEb/qvRstre0Egu967ILmqoqKfufyNT4W5JnWngliN6
S7D9cvxpW0RsUQHZXMqZp7s6kt4hAdziuyC2Wx2y6+zFHkbOwJcaULYrSNHJCPOj
cMu5TIplum+hnO9rMHKEpE0fAwKBgQC3h17rEy4uFbWPQD3fNjAi9QzIKX9wm8eD
SYekgZaHpAjrLCa8oNR6qMxU5Cpn7I3o2HegSUe29jDAr8GMp47JYRTGMHUl1Zwa
KtSGEH19sRhVUqIVW2h2/tysuaYpK1hFjPWM+KpKQFNzgt2EPf5057zE7gOHLcAL
UccMgP1crQKBgQCy4h1SaHrYZHq3LoNRwli6thrRc9YuoH4taXD+uuaSTvZE/gWv
H7hrwWcQ/mli229PJ1PspKc/HWMmE2giR669jCEwsMrHu/kYzjNE4oBfcYQNfhp4
RzVLtlHDdFM226KPixnCLThDK35x14YdqHxiixnyzqW8/g6a5mBHIBeVswKBgQCT
y79DndGdqTvqHbj1zWScci0V8F1BqSHVd1x1vSolF5NbF9YmJ3qVQOQ0JP6FbHmn
ntNPUFQhYkdGlQNQKwuQ3s5lAFcG3ev1IrK9OABnPTu0UnRWsKMC2SGLM4I9Ozu9
3tNL8GDqpLzPk/6h5W7KZGifSnGq5cv3EaczSZk/jQKBgAcaLGi25ozeFgK1qvuQ
WFTjLYV6KaMrGd5+NF+2a/NQsDGTZSF1egKUvE5QH5YNf37xWkqwvR3rsbenxLAG
aNYjvX+bUs4Mc/bgNkO51P9sH6YoKsuFzTTx4eR5ZS+dtfoiZMfzKkRBK4Baggrv
7S9Q3thVBhvBcz19oFN2Rmvf
-----END PRIVATE KEY-----`;
const cert = `-----BEGIN CERTIFICATE-----
MIIDJTCCAg2gAwIBAgIUZF3wbyk6BduDu+lEeegKd2ULMK8wDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDUyNDE3NDI1NloXDTM2MDUy
MTE3NDI1NlowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAjekpyhiK0q4H8TQo01JTA564FZpOitgwvIYMe3qhf0dF
lo2CbjxJcx5GOQ57k6vcNlLfIL2yV8f7hJNuFlfLAFvtm9pm45BvbsPvduW1AuSI
3oA/fpfsQ5K1VgAPbLZFhdndCjoGW3/ZO8PbUC5DTge5luCfXoV0zFzZATbJxziy
QZ9nYspc58Se6Xj0KhM3XCy2S7V5wVPRXo2nIW5ho83yHfKyVyKEew7nloxhrNAY
iRwHzVzBvCotdgZK/lBm1qsugHs31LR6T75izQGooIN1wz2V9kiHCW+s3CmgFCy2
5AbN547xLwn4djh5Tz4lJhA4Rh0D0F/vzceL5ToJBwIDAQABo28wbTAdBgNVHQ4E
FgQU1s+brNmcdkCqkncnW6rNlJpdiP0wHwYDVR0jBBgwFoAU1s+brNmcdkCqkncn
W6rNlJpdiP0wDwYDVR0TAQH/BAUwAwEB/zAaBgNVHREEEzARgglsb2NhbGhvc3SH
BH8AAAEwDQYJKoZIhvcNAQELBQADggEBAHFmvSxFCTHcqiocEHF3i0seBmNwWq40
TtyVf9qyZYUZVqM/Z7tGDsNfNOhM+YscLs1ZTs8XzdpdYBEVyCLDYGjb4Cv6r5gS
hr+E0NQBnPuker6Rw64nzahfWYjf/Eo+7nwUbCahTbXHAs43c4m0bmL02r1NxVmv
BKGQKO/uR9Dy+3TKykNQkacKJ6oDxdTDovMUKlbwU/HlyzwK/HTm762cJfgZiMYM
uru8x9wmqogCQSAz2q6a6q/CZfn1o7S5KiWd0FzinP+50g5cSL/ob0GJ8Jge1oI5
5rap/3DFfnTn0zfJ60U52+BVFnOIqkYT7/g5N4laGrza73tYXq7FV4s=
-----END CERTIFICATE-----`;

const client = String.raw`
import net from 'node:net';
import tls from 'node:tls';
const [port, mode, protocol] = process.argv.slice(1);
const started=performance.now();
const socket=protocol==='https' ? tls.connect({port:Number(port),host:'127.0.0.1',rejectUnauthorized:false}) : net.connect({port:Number(port),host:'127.0.0.1'});
let wire='',closed=false,pulse;
socket.on('data',b=>wire+=b.toString());socket.on('error',()=>{});
await new Promise(r=>socket.once(protocol==='https'?'secureConnect':'connect',r));
if(mode==='head'){socket.write('GET / HTTP/1.1\r\nHost: ');pulse=setInterval(()=>socket.write('x'),10);}
else if(mode==='body'){socket.write('POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n');pulse=setInterval(()=>socket.write('x'),10);}
else if(mode!=='empty'&&mode!=='zero')socket.write('GET / HTTP/1.1\r\nHost: x\r\nConnection: '+(mode==='keep'?'keep-alive':'close')+'\r\n\r\n');
if(mode==='peer')setTimeout(()=>socket.destroy(),80);
await new Promise(r=>{const timer=setTimeout(r,800);socket.once('close',()=>{closed=true;clearTimeout(timer);r();});});
clearInterval(pulse);
const elapsed=performance.now()-started;
const status=Number(/^HTTP\/1\.1 (\d+)/.exec(wire)?.[1]??0);
const body=wire.slice(wire.indexOf('\r\n\r\n')+4);
const ok=mode==='stream'?status===200&&!closed&&body.includes('heartbeat'):
 mode==='late'?status===200&&closed&&body==='late-response':
 mode==='normal'?status===200&&closed&&body==='ok':
 mode==='keep'?status===200&&closed&&body==='ok'&&elapsed<200:
 mode==='zero'?!closed&&status===0:
 mode==='peer'||mode==='force'?closed&&status===200:
 status===408&&closed&&body==='';
socket.destroy();
console.log(JSON.stringify({ok,status,closed}));
`;

async function run(protocol: string, mode: string) {
  const timers: any[] = [];
  const pulse = setInterval(() => {}, 10);
  const options: any = {requestTimeout:300,headersTimeout:100,connectionsCheckingInterval:10,keepAliveTimeout:20,keepAliveTimeoutBuffer:5};
  if(mode==='zero'){options.requestTimeout=0;options.headersTimeout=0;}
  if(protocol==='https'){options.key=key;options.cert=cert;}
  const handler = (_req: any,res: any) => {
    if(mode==='stream'||mode==='peer'||mode==='force'){
      res.writeHead(200,{'content-type':'text/event-stream'});
      res.write('data: open\n\n');
      const timer=setInterval(()=>res.write(': heartbeat\n\n'),10);
      timers.push(timer);res.on('close',()=>clearInterval(timer));
      if(mode==='force')timers.push(setTimeout(()=>server.closeAllConnections(),120));
    } else if(mode==='late'||mode==='body')timers.push(setTimeout(()=>res.end('late-response'),450));
    else res.end('ok');
  };
  const server: any = protocol==='https' ? https.createServer(options,handler) : http.createServer(options,handler);
  await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
  const result: any = await new Promise((resolve,reject)=>{
    const child=spawn('node',['--input-type=module','-e',client,String(server.address().port),mode,protocol]);
    let out='',err='';
    child.stdout.on('data',(b:any)=>{out+=b.toString();});child.stderr.on('data',(b:any)=>{err+=b.toString();});
    child.on('close',(code:number)=>{if(code!==0)return reject(new Error(err));try{resolve(JSON.parse(out));}catch(e){reject(e);}});
  });
  clearInterval(pulse);for(const timer of timers)clearTimeout(timer);
  server.closeAllConnections();
  await new Promise<void>(resolve=>server.close(resolve));
  console.log(protocol,mode,result.ok);
  if(!result.ok)throw new Error('HTTP timeout lifecycle '+protocol+' '+mode);
}

const watchdog=setTimeout(()=>{console.log('FAIL watchdog');process.exit(2);},30000);
for(const protocol of ['http','https'])for(const mode of ['stream','late','empty','head','body','zero','normal','keep','peer','force']){
  const label=protocol+':'+mode;
  if(process.argv[2]&&process.argv[2]!==label)continue;
  await run(protocol,mode);
}
const custom:any=http.createServer({requestTimeout:80});
const disabled:any=http.createServer({requestTimeout:0});
const explicit:any=http.createServer({requestTimeout:0,headersTimeout:40});
const defaults=[custom.headersTimeout===80,disabled.headersTimeout===0,explicit.headersTimeout===40];
console.log('defaults',...defaults);
if(defaults.some((ok:boolean)=>!ok))throw new Error('HTTP timeout defaults');
clearTimeout(watchdog);
