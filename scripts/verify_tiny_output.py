"""Exercise the shipped tiny output helper with deterministic syscall faults."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
helper = root / 'crates/perry/src/commands/compile/tiny_program/output.c'
out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='perry-tiny-output-faults-'))
main = work / 'main.c'
main.write_text(r'''
#include <stddef.h>
struct perry_tiny_output { int fd; const unsigned char *bytes; size_t length; };
void perry_tiny_run_output(const struct perry_tiny_output *, size_t);
#ifdef INJECT
int perry_test_verdict(void);
#endif
int main(void) {
    const struct perry_tiny_output outputs[] = {
        {1, (const unsigned char *)"alpha\n", 6},
        {2, (const unsigned char *)"beta\n", 5},
        {1, (const unsigned char *)"gamma\n", 6},
    };
    perry_tiny_run_output(outputs, 3);
#ifdef INJECT
    return perry_test_verdict();
#else
    return 0;
#endif
}
''')
faults = work / 'faults.c'
faults.write_text(r'''
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <poll.h>
#include <unistd.h>
static int mask, calls, polls;
ssize_t perry_test_write(int fd, const void *bytes, size_t length) {
    if (fd == 1 && calls++ == 0) { mask |= 1; errno = EINTR; return -1; }
    if (fd == 1 && calls == 2) { mask |= 2; errno = EAGAIN; return -1; }
    if (length > 3) { mask |= 4; length = 3; }
    return write(fd, bytes, length);
}
int perry_test_poll(struct pollfd *fds, nfds_t count, int timeout) {
    if (polls++ == 0) { mask |= 8; errno = EINTR; return -1; }
    return poll(fds, count, timeout);
}
int perry_test_verdict(void) { return mask == 15 ? 0 : 91; }
''')
receipt = {'source': str(helper), 'sourceSha256': hashlib.sha256(helper.read_bytes()).hexdigest(),
           'work': str(work), 'commands': [], 'cases': {}, 'complete': False, 'passed': False}

def run(argv):
    result = subprocess.run([str(x) for x in argv], capture_output=True, timeout=30)
    receipt['commands'].append({'argv': [str(x) for x in argv], 'exitCode': result.returncode,
                                'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()})
    assert result.returncode == 0, receipt['commands'][-1]
    return result

try:
    for label, injected in [('normal', False), ('partial-eintr-eagain-poll-eintr', True)]:
        obj = work / (label + '.o')
        argv = ['cc', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-c', helper, '-o', obj]
        if injected:
            argv += ['-Dwrite=perry_test_write', '-Dpoll=perry_test_poll']
        run(argv)
        binary = work / label
        argv = ['cc', '-std=c11', '-O2', main, obj, '-o', binary]
        if injected:
            argv += ['-DINJECT=1', faults]
        run(argv)
        observed = run([binary])
        assert observed.stdout == b'alpha\ngamma\n' and observed.stderr == b'beta\n'
        receipt['cases'][label] = {'passed': True, 'stdout': observed.stdout.decode(), 'stderr': observed.stderr.decode()}
    receipt['complete'] = True
    receipt['passed'] = True
finally:
    (out / 'verification.json').write_text(json.dumps(receipt, indent=2) + '\n')
