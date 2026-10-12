#!/usr/bin/env python3
"""Run a lane command on qb6 with the arm environment set after bash -lc."""
import os, shlex, subprocess, sys
arm = sys.argv[1]
base = '/root/codex-lanes/cx-allocstate' + ('/main' if arm == 'main' else '')
env = dict(PATH='/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH',
           CARGO_TARGET_DIR=base+'/target', PERRY_RUNTIME_DIR=base+'/target/release',
           PERRY_WORKSPACE_ROOT=base+'/src', RUST_TEST_THREADS='1',
           TMPDIR='/root/codex-lanes/cx-allocstate/tmp')
exports = 'export ' + ' '.join(k+'='+ (v if k=='PATH' else shlex.quote(v)) for k,v in env.items())
command = exports+'; '+sys.argv[2]
sys.exit(subprocess.call(['ssh','root@185.191.117.52','bash -lc '+shlex.quote(command)]))
