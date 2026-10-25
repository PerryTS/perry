#!/usr/bin/env python3
"""Exercise P3 HTTP from compiled Perry TypeScript with real suspended imports."""
import argparse
import asyncio
import http.client
import json
import os
from pathlib import Path
import queue
import re
import shutil
import ssl
import subprocess
import threading
import time


async def client(case, address, body, tls, certificates):
    host, port = address.rsplit(':', 1)
    context = ssl.create_default_context(cafile=str(certificates / 'test-ca.pem')) if tls else None
    reader, writer = await asyncio.open_connection(host, int(port), ssl=context, server_hostname='localhost' if tls else None)
    async def upload():
        writer.write(b'POST /echo HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n')
        for n in range(0, len(body), 16381):
            chunk = body[n:n+16381]
            writer.write(f'{len(chunk):x}\r\n'.encode() + chunk + b'\r\n')
            await writer.drain()
        writer.write(b'0\r\nx-native: preserved\r\n\r\n')
        await writer.drain()
    uploading = asyncio.create_task(upload())
    status = None
    try:
        line = await reader.readline()
        if not line: raise http.client.RemoteDisconnected()
        assert line.startswith(b'HTTP/1.1 '), line
        status = int(line.split()[1])
        headers = {}
        while True:
            line = await reader.readline()
            if not line: raise http.client.RemoteDisconnected()
            if line == b'\r\n': break
            key, value = line.split(b':', 1)
            headers[key.lower()] = value.strip()
        data, trailers = bytearray(), []
        if headers.get(b'transfer-encoding') == b'chunked':
            while True:
                line = await reader.readline()
                if not line: raise http.client.RemoteDisconnected()
                n = int(line.strip().split(b';')[0], 16)
                if not n:
                    while True:
                        line = await reader.readline()
                        if not line: raise http.client.RemoteDisconnected()
                        if line == b'\r\n': break
                        trailers.append(line.strip())
                    break
                data.extend(await reader.readexactly(n))
                assert await reader.readexactly(2) == b'\r\n'
        elif b'content-length' in headers:
            data.extend(await reader.readexactly(int(headers[b'content-length'])))
        else:
            data.extend(await reader.read())
        await uploading
        if case in ['empty', 'large', 'reused']:
            assert status == 200 and bytes(data) == body, (case, status, len(data))
            assert trailers == [b'x-native: preserved'], trailers
        elif case in ['denied', 'denied-reused', 'trap', 'reject', 'dispose', 'close']:
            assert status == 500, (case, status)
        elif case == 'late-trap':
            assert status in [200, 500], status
        else:
            raise AssertionError('cancelled exchange unexpectedly returned a response: ' + case)
        return dict(case=case, status=status, bodyBytes=len(data), trailers=[t.decode() for t in trailers])
    except (http.client.RemoteDisconnected, ConnectionResetError, asyncio.IncompleteReadError, ssl.SSLError):
        assert case in ['late-trap', 'reject', 'dispose', 'close'], case
        return dict(case=case, status=status, disconnected=True)
    finally:
        if not uploading.done(): uploading.cancel()
        await asyncio.gather(uploading, return_exceptions=True)
        writer.close()
        try: await writer.wait_closed()
        except (OSError, ssl.SSLError): pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['perry', 'runtime-dir', 'wit', 'wat-encoder', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--moving-gc', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for name in ['host.ts', 'world.wit']:
        shutil.copyfile(root / 'test-wasi/components/http-p3' / name, out / name)
    env = os.environ | {'PERRY_RUNTIME_DIR': str(args.runtime_dir.resolve()),
                        'PERRY_NO_AUTO_OPTIMIZE': '1', 'PERRY_CODEGEN_CPU_SLOTS': '4'}
    rows = []

    def record(name, status, **details):
        rows.append(dict(phase=name, exitCode=status, **details))
        (out / 'results.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(('PASS ' if not status else 'FAIL ') + name, flush=True)
        if status:
            raise SystemExit(status)

    def run(name, command, run_env):
        with (out / (name + '.log')).open('w') as log:
            p = subprocess.run(list(map(str, command)), cwd=out, env=run_env,
                               stdout=log, stderr=subprocess.STDOUT, timeout=600)
        record(name, p.returncode)

    run('bindings', [args.wit.resolve(), out / 'world.wit', out / 'generated'], env)
    source = (root / 'crates/perry-component-host/src/http/p3/bridge.component.wat').read_text()
    denied = source.replace('  (core module $guest', '  (core func $drop-request (canon resource.drop $request))\n  (core module $guest\n    (import "host" "drop-request" (func $drop-request (param i32)))')
    denied = denied.replace('(with "host" (instance', '(with "host" (instance (export "drop-request" (func $drop-request))')
    start = denied.index('    (func $original-handle')
    end = denied.index('    (func $original-callback', start)
    denied = denied[:start] + '''    (func $original-handle (param $request i32) (result i32)
      (call $drop-request (local.get $request))
      (call $return (i32.const 1) (i32.const 15) (i32.const 0) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)) (i32.const 0))
''' + denied[end:]
    sources = {'echo': source, 'denied': denied,
               'trap': source.replace('(global.set $set (call $new-set))', 'unreachable', 1),
               'late-trap': source.replace('(i32.const 0))\n    (func $original-handle', '(i32.const 1))\n    (func $original-handle')}
    assert sources['late-trap'] != source
    for name, wat in sources.items():
        (out / (name + '.wat')).write_text(wat)
        run('encode-' + name, [args.wat_encoder.resolve(), out / (name + '.wat'), out / (name + '.wasm')], env)
    cases = ['empty', 'large', 'reused', 'denied', 'denied-reused', 'trap', 'late-trap', 'reject', 'dispose', 'close']
    payload = bytes((n * 23 + 59) % 256 for n in range(3 * 1024 * 1024 + 137))
    for moving in ([False, True] if args.moving_gc else [False]):
        mode = 'moving' if moving else 'ordinary'
        knobs = {k: '1' for k in ['PERRY_GC_INSTRUMENTS', 'PERRY_GC_FORCE_EVACUATE', 'PERRY_GC_VERIFY_EVACUATION', 'PERRY_GC_PROTECT_FROMSPACE', 'PERRY_GC_DIAG']} if moving else {}
        run_env = env | knobs
        executable = out / ('host-' + mode)
        run('compile-' + mode, [args.perry.resolve(), 'compile', out / 'host.ts', '--no-cache', '--no-auto-optimize', '-o', executable], run_env)
        certificates = root / 'crates/perry-tls-session/tests'
        for tls in [False, True]:
            name = ('tls-' if tls else 'plain-') + mode
            events = queue.Queue()
            responses = []
            with (out / (name + '-stderr.log')).open('w') as errors, (out / (name + '.log')).open('w') as log:
                process = subprocess.Popen([str(executable), str(out), 'tls' if tls else 'plain', str(certificates / 'test-cert.pem'), str(certificates / 'test-key.pem')], cwd=out, env=run_env, text=True, stdout=subprocess.PIPE, stderr=errors, bufsize=1)
                def consume():
                    for line in process.stdout:
                        events.put(line)
                    events.put(None)
                reader = threading.Thread(target=consume, daemon=True)
                reader.start()
                completed = []
                final = False
                failure = None
                deadline = time.monotonic() + 90
                try:
                    while True:
                        line = events.get(timeout=max(0.1, deadline - time.monotonic()))
                        if line is None:
                            break
                        log.write(line); log.flush()
                        if line.startswith('READY '):
                            _, case, address = line.strip().split()
                            assert case == cases[len(responses)], case
                            body = payload if case == 'large' else (b'' if case == 'empty' else b'pong')
                            responses.append(asyncio.run(asyncio.wait_for(client(case, address, body, tls, certificates), 15)))
                        elif line.startswith('RESULT '):
                            completed.append(line.strip().split()[1])
                        elif line.startswith('PASS P3 HTTP:'):
                            final = True
                    status = process.wait(timeout=max(0.1, deadline-time.monotonic()))
                    assert status == 0 and final and completed == cases and len(responses) == len(cases), (status, final, completed)
                except (AssertionError, OSError, ValueError, TimeoutError, queue.Empty, subprocess.TimeoutExpired, http.client.HTTPException) as error:
                    failure = str(error) or type(error).__name__
                    if process.poll() is None: process.kill()
                    process.wait(); status = 1
                finally:
                    process.stdout.close(); reader.join(timeout=1)
            diagnostics = (out / (name + '-stderr.log')).read_text()
            copied = sum(map(int, re.findall(r'\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)', diagnostics)))
            if moving and (copied == 0 or 'evacuation_ok' not in diagnostics or not re.search(r'mode=ProtectPages retired_set=#\d+[^\n]*\bbytes_protected=[1-9]\d*', diagnostics)):
                status, failure = 1, 'moving GC and protected from-space witnesses required'
            record(name, status, failure=failure, responses=responses, copiedObjects=copied)


if __name__ == '__main__':
    main()
