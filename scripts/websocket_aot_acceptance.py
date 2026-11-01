#!/usr/bin/env python3
"""Test Perry against a pinned, unmodified upstream WebSocket AOT state machine.

Provide a checkout of guest271314/websocket-server at UPSTREAM_COMMIT. This is
functional acceptance; elapsed smoke times and artifact sizes are not a fair
compiler performance comparison. Run heavy builds on the qualification host.
"""
import argparse
from pathlib import Path
import base64
import hashlib
import json
import os
import shutil
import socket
import struct
import subprocess
import time

UPSTREAM_COMMIT = "d7f94f5634f733415f1f52547fe45b3794d9d41a"
SOURCE_SHA256 = "ea9269e655def8f0aa95c09c467a5cb048f71e55994d22d1c90285f513b5de49"
ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
for option in ['perry', 'native-runtime', 'p1-runtime', 'p3-runtime', 'p3-contract', 'upstream', 'output']:
    parser.add_argument('--' + option, type=Path, required=True)
parser.add_argument('--live-clients', action='store_true',
    help='also run both unmodified upstream clients over real sockets; requires free loopback port 8080')
args = parser.parse_args()
upstream = args.upstream.resolve()
source_input = upstream / 'websocket-server-wasm-debug.ts'
if hashlib.sha256(source_input.read_bytes()).hexdigest() != SOURCE_SHA256:
    parser.error('upstream TypeScript does not match pinned commit ' + UPSTREAM_COMMIT)
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=False)
(out / 'OWNER').write_text('Pinned upstream WebSocket AOT functional acceptance. Preserve.\n')
(out / 'source').mkdir()
shutil.copy2(source_input, out / 'source' / source_input.name)
shutil.copy2(upstream / 'LICENSE', out / 'source' / 'LICENSE')
env = os.environ | {'PERRY_RUNTIME_DIR': str(args.native_runtime.resolve()),
    'PERRY_CODEGEN_CPU_SLOTS': '4'}
perry = args.perry.resolve()
rows=[]
def run(name,args,data=None,expected=None,extra=None,timeout=300):
    print('START',name,flush=True)
    start=time.monotonic()
    r=subprocess.run(list(map(str,args)),cwd=out,env={**env,**(extra or {})},input=data,capture_output=True,timeout=timeout)
    (out/(name+'.stdout')).write_bytes(r.stdout);(out/(name+'.stderr')).write_bytes(r.stderr)
    passed=r.returncode==0 and (expected is None or r.stdout==expected)
    rows.append(dict(phase=name,exitCode=r.returncode,passed=passed,seconds=time.monotonic()-start,stdoutBytes=len(r.stdout),expectedBytes=len(expected) if expected is not None else None))
    (out/'phases.json').write_text(json.dumps(rows,indent=2)+'\n')
    print('DONE',name,passed,r.returncode,len(r.stdout),flush=True)
    if not passed:
        (out/'result.json').write_text(json.dumps(dict(exitCode=r.returncode or 1,failedPhase=name,performanceQualified=False),indent=2)+'\n')
        print(r.stderr.decode(errors='replace')[-7000:],flush=True)
        if expected is not None:
            mismatch=next((i for i,(a,c) in enumerate(zip(r.stdout,expected)) if a!=c),min(len(r.stdout),len(expected)))
            print('FIRST DIFFERENCE',mismatch,'ACTUAL',r.stdout[mismatch:mismatch+120],'EXPECTED',expected[mismatch:mismatch+120],flush=True)
        raise SystemExit(r.returncode or 1)
    return r
key=b'dGhlIHNhbXBsZSBub25jZQ=='
request=b'GET / HTTP/1.1\r\nHost: localhost:8080\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: '+key+b'\r\nSec-WebSocket-Version: 13\r\n\r\n'
accept=base64.b64encode(hashlib.sha1(key+b'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest())
response=b'HTTP/1.1 101 Web Socket Protocol Handshake\r\nUpgrade: WebSocket\r\nConnection: Upgrade\r\nSec-Websocket-Accept: '+accept+b'\r\n\r\n'
assert accept==b's3pPLMBiTxaQ9kYGzzhZRbK+xOo='
def frame(op,payload,masked):
    n=len(payload);header=bytes([128|op,(128 if masked else 0)|(n if n<126 else 126 if n<65536 else 127)])
    if n>=126:header+=struct.pack('>H' if n<65536 else '>Q',n)
    if masked:
        mask=b'\x11\x22\x33\x44'
        return header+mask+bytes(value^mask[i%4] for i,value in enumerate(payload))
    return header+payload
def case(items):
    close=struct.pack('>H',1000)+b'Done'
    incoming=request+b''.join(frame(op,p,True) for op,p in items)+frame(8,close,True)
    outgoing=response+b''.join(frame(10 if op==9 else op,p,False) for op,p in items if op!=10)+frame(8,close,False)
    return incoming,outgoing
cases={
    'text-control':case([(1,b'hello perry'),(2,b'\0\x80\xff'),(9,b'ping'),(10,b'ignored')]),
    'length-boundaries':case([(2,bytes((i*17+31)%256 for i in range(n))) for n in [0,125,126,127,65535,65536,65537,1048576]]),
    'binary-20mib':case([(2,b'\1'*65536) for _ in range(320)]),
}
for name,(incoming,expected) in cases.items():
    (out/(name+'.input')).write_bytes(incoming);(out/(name+'.expected')).write_bytes(expected)
source=out/'source/websocket-server-wasm-debug.ts'
run('node-version',['node','--version'])
assert (out/'node-version.stdout').read_text().strip()=='v26.12.1'
for name,(incoming,expected) in cases.items():run('node-'+name,['node','--experimental-strip-types',source],incoming,expected)
native=out/'websocket-native'
run('native-compile',[perry,'compile',source,'--no-auto-optimize','--no-cache','-o',native])
for name,(incoming,expected) in cases.items():run('native-'+name,[native],incoming,expected)
component=out/'websocket-p3.component.wasm'
run('p3-compile',[perry,'compile',source,'--target','wasip3','--no-auto-optimize','--no-cache','-o',component],extra={'PERRY_RUNTIME_DIR':str(args.p3_runtime.resolve())})
run('p3-contract',[args.p3_contract.resolve(),component])
host_source=out/'p3-host.ts'
host_source.write_text('import { readFileSync } from "node:fs";\nimport { runComponentCommand } from "perry/component";\nprocess.exit(runComponentCommand(readFileSync(process.argv[2]), { fuel: 1000000000000, memoryBytes: 536870912, wasi: { inheritStdin: true, inheritStdout: true, inheritStderr: true } }));\n')
host=out/'p3-host'
run('p3-host-compile',[perry,'compile',host_source,'--no-auto-optimize','--no-cache','-o',host])
for name,(incoming,expected) in cases.items():run('p3-'+name,[host,component],incoming,expected)
p1=out/'websocket-p1.wasm'
run('p1-compile',[perry,'compile',source,'--target','wasip1','--no-auto-optimize','--no-cache','-o',p1],extra={'PERRY_RUNTIME_DIR':str(args.p1_runtime.resolve())})
run('p1-contract',['python3',ROOT/'scripts/wasi_imports_check.py',p1])
p1host=out/'p1-host.mjs'
p1host.write_text('import { WASI } from "node:wasi";\nimport { readFileSync } from "node:fs";\nconst wasi = new WASI({ version: "preview1", args: [], env: {}, returnOnExit: true });\nconst module = await WebAssembly.compile(readFileSync(process.argv[2]));\nconst instance = await WebAssembly.instantiate(module, wasi.getImportObject());\nprocess.exitCode = wasi.start(instance);\n')
for name,(incoming,expected) in cases.items():run('p1-'+name,['node',p1host,p1],incoming,expected)
if args.live_clients:
    clients = {
        'client.mjs': 'a60a32a1bad72ce416a032ca05b2bb5e27e0b25b4b8a688aa180eab3219ebc82',
        'websocket-stream-client.js': 'f54f41843ef41192a4276f7dbbd0889faf410410cf16a88d5d2aae3d88bb363b',
    }
    commands = {'node': ['node', '--experimental-strip-types', source],
        'native': [native], 'p1': ['node', p1host, p1], 'p3': [host, component]}
    for mode, command in commands.items():
        for kind, client_name in [('text', 'client.mjs'), ('binary', 'websocket-stream-client.js')]:
            client = upstream / client_name
            if hashlib.sha256(client.read_bytes()).hexdigest() != clients[client_name]:
                raise SystemExit('upstream client differs from pinned commit: ' + client_name)
            name = 'socket-' + mode + '-' + kind
            print('START', name, flush=True)
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                # The binary client fixes its URL at 8080. Keep its source intact.
                listener.bind(('127.0.0.1', 0 if kind == 'text' else 8080))
                listener.listen(1)
                listener.settimeout(10)
                port = listener.getsockname()[1]
                with (out / (name + '.client.stdout')).open('wb') as co, \
                     (out / (name + '.client.stderr')).open('wb') as ce, \
                     (out / (name + '.server.stderr')).open('wb') as se:
                    client_process = subprocess.Popen(['node', str(client), str(port)],
                        cwd=out, env=env, stdout=co, stderr=ce)
                    server = None
                    try:
                        conn, _ = listener.accept()
                        with conn:
                            conn.setblocking(True)
                            server = subprocess.Popen(list(map(str, command)), cwd=out,
                                env=env, stdin=conn, stdout=conn, stderr=se)
                        client_status = client_process.wait(timeout=120)
                        server_status = server.wait(timeout=30)
                    finally:
                        for child in [server, client_process]:
                            if child is not None and child.poll() is None:
                                child.kill()
                                child.wait()
            client_output = (out / (name + '.client.stdout')).read_text()
            passed = client_status == server_status == 0
            passed = passed and 'Assertion failed' not in (out / (name + '.client.stderr')).read_text()
            if kind == 'text':
                passed = passed and 'message: hello jz' in client_output and 'close 1000 Done' in client_output
            else:
                passed = passed and all(text in client_output for text in
                    ['binaryResult: 20971520', 'textResult: 4', '4999', 'Done streaming'])
            rows.append(dict(phase=name, clientExit=client_status, serverExit=server_status, passed=passed))
            (out / 'phases.json').write_text(json.dumps(rows, indent=2) + '\n')
            print('DONE', name, passed, client_status, server_status, flush=True)
            if not passed:
                (out / 'result.json').write_text(json.dumps(dict(exitCode=1,
                    failedPhase=name, performanceQualified=False), indent=2) + '\n')
                print(client_output, (out / (name + '.server.stderr')).read_text(), flush=True)
                raise SystemExit(1)
identities={str(p):dict(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in [source,native,component,host,p1,p1host,perry,*(path.resolve()/'libperry_runtime.a' for path in [args.native_runtime,args.p1_runtime,args.p3_runtime])]}
(out/'artifact-identities.json').write_text(json.dumps(identities,indent=2)+'\n')
(out/'result.json').write_text(json.dumps(dict(exitCode=0,upstreamCommit=UPSTREAM_COMMIT,sourceUnmodified=True,profile='supplied compiler and runtime archives, functional acceptance only',performanceQualified=False,preview1Qualified=True,liveClientsQualified=args.live_clients,remaining=([] if args.live_clients else ['live socket/client roundtrip'])+['production release size/startup/throughput/memory comparisons']),indent=2)+'\n')
print('WEBSOCKET NATIVE, PREVIEW 1 AND P3 FUNCTIONAL ACCEPTANCE PASS',flush=True)
