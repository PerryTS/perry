"""Verify automatic tiny selection, Node output, symbols, and pipe behavior.

Run outside a source checkout when testing the installed npm compiler. Optional
--build-env supplies explicit archive paths for a source-built compiler; no flag
or environment variable enables the tiny optimization. Full fallback fixtures
execute by default; --tiny-only is for an early compiler-only smoke test.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import tempfile
import threading
import time

parser = argparse.ArgumentParser()
parser.add_argument('compiler')
parser.add_argument('output')
parser.add_argument('--build-env', default='{}')
parser.add_argument('--tiny-only', action='store_true')
args = parser.parse_args()
compiler = str(Path(args.compiler).resolve())
out = Path(args.output).resolve()
out.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='perry-tiny-oracles-'))
(work / 'package.json').write_text('{"type":"module"}\n')
env = {k: v for k, v in os.environ.items() if not k.upper().startswith(('PERRY_', 'MIMALLOC_', 'DYLD_', 'LD_PRELOAD'))}
build_env = {**env, **json.loads(args.build_env), 'PERRY_KEEP_SYMBOLS': '1'}

def file_record(p):
    p = Path(p)
    return {'file': str(p), 'bytes': p.stat().st_size, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}

def captured(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
            **({'text': data.decode('utf-8', errors='backslashreplace')} if len(data) < 4096 else {})}

result = {'compiler': file_record(compiler), 'verifier': file_record(__file__), 'work': str(work),
          'buildEnv': json.loads(args.build_env), 'node': subprocess.check_output(['node', '-v'], text=True).strip(),
          'complete': False, 'passed': False, 'scope': 'tiny-only' if args.tiny_only else 'tiny-and-fallback', 'fixtures': {}}
assert result['node'] == 'v26.5.1', 'Use the pinned Node 26.5.1 oracle'

def save():
    (out / 'verification.json').write_text(json.dumps(result, indent=2) + '\n')

def run(command, **kwargs):
    return subprocess.run(command, env=env, capture_output=True, timeout=45, **kwargs)

positive = {
    'empty': '// empty\n;',
    'literal': 'console.log("hello");',
    'hello': 'const who: string = "world"; console.log(`hello, ${who}`); console.log("RESULT 1");',
    'stderr': 'console.error("error");',
    'ordering': 'console.log("one"); console.error("two"); console.log("three"); console.error("four");',
    'nul-unicode': r'console.log("a\0b\n🦀\uD800"); console.error("%s %d %% é");',
    'surrogate-join': r'const low = "\uDC00"; console.log(`\uD800${low}`);',
    'empty-string': 'console.log(""); console.error("");',
    'constant-chain': 'const a="a", b=`${a}b`; const c=`${b}${a}`; console.log(c);',
    'assertions': 'const s = "value"; console.log((s as string)!);',
    'escaped-builtin': r'\u0063onsole.\u006cog("escaped");',
    'pipe-survival': 'console.log("first"); console.log("second"); console.error("survived");',
    'stdout-handshake': 'const s="' + 'x' * 131072 + '"; console.log(s); console.error("ready");',
    'stderr-handshake': 'const s="' + 'x' * 131072 + '"; console.error(s); console.log("ready");',
    'backpressure': 'const s = "' + 'x' * 512 + '";\n' + 'console.log(s);\n' * 512,
}
fallback = {
    'sync-allocation-loop': 'let a: any[] = []; for(let i=0;i<100;i++) a.push({i}); console.log(a.length);',
    'numeric': 'console.log(42);',
    'arguments': 'console.log("a", "b");',
    'promise': 'Promise.resolve().then(() => console.log("later")); console.log("first");',
    'timer': 'setTimeout(() => console.log("timer"), 0);',
    'lifecycle': 'process.on("exit", () => console.log("exit")); console.log("body");',
    'computed-console': 'console["log"]("computed");',
    'global-alias': 'globalThis.console.log("global");',
    'dead-code': 'if (false) { new Promise(() => {}); } console.log("kept");',
    'unused-function': 'function unused() {} console.log("kept");',
    'exceptions': 'try { throw new Error("caught"); } catch(e) { console.log(e.message); }',
    'import': 'import { basename } from "node:path"; console.log(basename("/a/b"));',
    'getters': 'const x={get value(){console.log("get");return "value";}}; console.log(x.value);',
}
# Receiver replacement cases are covered by eligibility unit tests; the known
# baseline console-replacement defect is not disguised as a new Node pass.
try:
    for name, source in {**positive, **({} if args.tiny_only else fallback)}.items():
        src = work / (name + '.ts')
        src.write_text(source + '\n')
        binary = work / (name + '.bin')
        row = result['fixtures'][name] = {'source': file_record(src), 'expectedProfile': 'tiny' if name in positive else 'normal'}
        argv = [compiler, 'compile', str(src), '--no-cache', '--keep-intermediates', '-v', '-o', str(binary)]
        build = subprocess.run(argv, cwd=work, env=build_env, capture_output=True, timeout=240)
        log = out / (name + '-build.log')
        log.write_bytes(build.stdout + build.stderr)
        row['build'] = {'argv': argv, 'exitCode': build.returncode, 'log': file_record(log)}
        assert build.returncode == 0, f'{name}: compile failed; see {log}'
        selected = b'using proven tiny program:' in build.stdout + build.stderr
        assert selected == (name in positive), f'{name}: incorrect automatic selection'
        row['tinySelected'] = selected
        row['binary'] = file_record(binary)
        oracle = run(['node', str(src)])
        actual = run([str(binary)])
        row['node'] = {'exitCode': oracle.returncode, 'stdout': captured(oracle.stdout), 'stderr': captured(oracle.stderr)}
        row['native'] = {'exitCode': actual.returncode, 'stdout': captured(actual.stdout), 'stderr': captured(actual.stderr)}
        assert oracle.returncode == 0, f'{name}: Node oracle did not execute'
        assert (actual.returncode, actual.stdout, actual.stderr) == (oracle.returncode, oracle.stdout, oracle.stderr), f'{name}: output mismatch'
        if selected:
            ir = binary.with_suffix('.tiny.ll')
            row['ir'] = file_record(ir)
            symbols = run(['nm', str(binary)])
            assert symbols.returncode == 0, f'{name}: nm failed'
            symbol_log = out / (name + '-symbols.log')
            symbol_log.write_bytes(symbols.stdout + symbols.stderr)
            row['symbols'] = file_record(symbol_log)
            forbidden = rb'js_gc_|js_promise_|js_array_|js_string_|mimalloc|mi_malloc|perry_runtime|shadow_frame'
            assert not re.search(forbidden, symbols.stdout), f'{name}: managed runtime symbols present'
            assert b'libperry_' not in build.stdout + build.stderr, f'{name}: runtime archive linked'
        row['passed'] = True
        save()
        print(name, 'passed', flush=True)

    # Machine-readable output keeps the ordinary compiler's success envelope.
    # Compare against a real normal build when full archives are available.
    result['jsonCli'] = {}
    for label in ['tiny'] + ([] if args.tiny_only else ['normal']):
        binary = work / ('json-' + label + '.bin')
        argv = [compiler, '--format', 'json', 'compile', str(work / 'literal.ts'),
                '--no-cache', '-o', str(binary)]
        if label == 'normal':
            argv.append('--no-auto-optimize')
        built = subprocess.run(argv, cwd=work, env=build_env, capture_output=True, timeout=240)
        assert built.returncode == 0, f'JSON {label} compile failed: {built.stderr!r}'
        response = json.loads(built.stdout)
        assert response['success'] is True and response['output'] == str(binary)
        assert response['native_modules'] == 1 and response['js_modules'] == 0
        assert {'build_cache', 'codegen_cache', 'link_cache'} <= response.keys()
        actual = run([str(binary)])
        assert (actual.returncode, actual.stdout, actual.stderr) == (0, b'hello\n', b'')
        result['jsonCli'][label] = {'argv': argv, 'response': response,
                                    'binary': file_record(binary), 'stderr': captured(built.stderr)}
    assert result['jsonCli']['tiny']['response']['runtimeProfile'] == 'tiny'
    if not args.tiny_only:
        normal = result['jsonCli']['normal']['response']
        tiny = result['jsonCli']['tiny']['response']
        assert normal.keys() <= tiny.keys(), 'Tiny dropped ordinary JSON result fields'
        assert normal['link_cache'].keys() == tiny['link_cache'].keys()
    save()

    pipe_source = work / 'pipe-survival.ts'
    pipe_binary = work / 'pipe-survival.bin'
    result['pipeCases'] = {}
    # Both descriptors point at the same pipe: verify cross-stream call order.
    def merged(command):
        return subprocess.run(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
    before = merged(['node', str(work / 'ordering.ts')])
    after = merged([str(work / 'ordering.bin')])
    assert (after.returncode, after.stdout) == (before.returncode, before.stdout)
    result['pipeCases']['merged-order'] = captured(after.stdout)

    def broken(command):
        readfd, writefd = os.pipe()
        os.close(readfd)
        try:
            child = subprocess.Popen(command, env=env, stdout=writefd, stderr=subprocess.PIPE)
        finally:
            os.close(writefd)
        _, stderr = child.communicate(timeout=30)
        return child.returncode, stderr
    before = broken(['node', str(pipe_source)])
    after = broken([str(pipe_binary)])
    assert before == after == (0, b'survived\n'), f'broken pipe: {before!r} / {after!r}'
    result['pipeCases']['broken-stdout'] = {'exitCode': after[0], 'stderr': captured(after[1])}

    def full_device(command):
        with open('/dev/full', 'wb', buffering=0) as sink:
            child = subprocess.run(command, env=env, stdout=sink, stderr=subprocess.PIPE, timeout=30)
        return child.returncode, child.stderr
    if Path('/dev/full').exists():
        before, after = full_device(['node', str(pipe_source)]), full_device([str(pipe_binary)])
        assert before == after == (0, b'survived\n')
        result['pipeCases']['full-device'] = {'exitCode': after[0], 'stderr': captured(after[1])}

    def backpressure(command):
        readfd, writefd = os.pipe()
        os.set_blocking(writefd, False)
        child = subprocess.Popen(command, env=env, stdout=writefd, stderr=subprocess.PIPE)
        os.close(writefd)
        chunks = []
        def consume():
            time.sleep(.15) # Ensure the inherited nonblocking pipe fills first.
            with os.fdopen(readfd, 'rb', buffering=0) as stream:
                while chunk := stream.read(4096):
                    chunks.append(chunk)
                    time.sleep(.001)
        reader = threading.Thread(target=consume, daemon=True)
        reader.start()
        _, stderr = child.communicate(timeout=30)
        reader.join(timeout=30)
        assert not reader.is_alive()
        return child.returncode, b''.join(chunks), stderr
    before = backpressure(['node', str(work / 'backpressure.ts')])
    after = backpressure([str(work / 'backpressure.bin')])
    assert after == before and after[0] == 0 and len(after[1]) == 513 * 512, 'nonblocking output lost bytes'
    result['pipeCases']['nonblocking-backpressure'] = {'exitCode': after[0], 'stdout': captured(after[1]), 'stderr': captured(after[2])}
    # A consumer waits for the other stream before draining the full one.
    # Blocking writes would deadlock this valid interaction; Node sends ready.
    for name, waiting_stream in [('stdout-handshake', 'stderr'), ('stderr-handshake', 'stdout')]:
        observed = {}
        for label, command in [('node', ['node', str(work / (name + '.ts'))]), ('tiny', [str(work / (name + '.bin'))])]:
            child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            with selectors.DefaultSelector() as selector:
                selector.register(getattr(child, waiting_stream), selectors.EVENT_READ)
                ready = bool(selector.select(timeout=3))
            stdout, stderr = child.communicate(timeout=30)
            observed[label] = {'readyBeforeDrain': ready, 'exitCode': child.returncode,
                               'stdout': captured(stdout), 'stderr': captured(stderr)}
        assert observed['node'] == observed['tiny'] and observed['node']['readyBeforeDrain'], f'{name}: independent streams stalled'
        result['pipeCases'][name] = observed

    def closed_stdout(command):
        child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 preexec_fn=lambda: os.close(1))
        stdout, stderr = child.communicate(timeout=30)
        return child.returncode, stdout, stderr
    before = closed_stdout(['node', str(pipe_source)])
    after = closed_stdout([str(pipe_binary)])
    assert before == after == (0, b'', b'survived\n'), f'closed stdout: {before!r} / {after!r}'
    result['pipeCases']['closed-stdout'] = {'exitCode': after[0], 'stderr': captured(after[2])}
    result['complete'] = True
    result['passed'] = True
except Exception as error:
    result['error'] = str(error)
    raise
finally:
    save()
