'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { detectPlatform, readHost, PLATFORM_PACKAGES } = require('../bin/detect.cjs');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-wit-launcher-'));
try {
  fs.mkdirSync(path.join(root, 'bin'));
  fs.writeFileSync(path.join(root, 'package.json'), '{"type":"commonjs"}');
  for (const file of ['perry-wit.js', 'detect.cjs']) {
    fs.copyFileSync(path.join(__dirname, '../bin', file), path.join(root, 'bin', file));
  }
  const launcher = path.join(root, 'bin/perry-wit.js');
  const run = (args = []) => spawnSync(process.execPath, [launcher, ...args], {
    cwd: root, input: 'stdin payload', encoding: 'utf8', timeout: 10000,
  });
  const missing = run();
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /No WIT tool is installed/);
  // This test executes a shebang fixture, so its CI home is the Unix launcher job.
  if (process.platform !== 'win32') {
    const key = detectPlatform(readHost()).candidates[0];
    const directory = path.join(root, 'node_modules', PLATFORM_PACKAGES[key], 'bin');
    fs.mkdirSync(directory, { recursive: true });
    const binary = path.join(directory, 'perry-wit');
    fs.writeFileSync(binary, '#!/usr/bin/env node\n' +
      'console.log(JSON.stringify({args:process.argv.slice(2),stdin:require("fs").readFileSync(0,"utf8")}));process.exit(7);\n', { mode: 0o755 });
    const result = run(['world with spaces.wit', 'output', 'plugin']);
    assert.equal(result.status, 7, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), {
      args: ['world with spaces.wit', 'output', 'plugin'], stdin: 'stdin payload',
    });
    fs.chmodSync(binary, 0o644);
    const invalid = run();
    assert.equal(invalid.status, 1);
    assert.match(invalid.stderr, /Failed to start/);
  }
  console.log('WIT launcher: missing tool, argv/stdin/exit propagation, spawn failure PASS');
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
