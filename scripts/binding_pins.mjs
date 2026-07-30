#!/usr/bin/env node
// Upstream provenance pins for well_known_bindings.toml — provisioning,
// lock-step gate, and porting helpers.
//
// Ported from the socket-registry fleet's upstream-reference technique
// (gitmodules-hash.mts provisions pins; vendor-actions.mts --check reds
// when a pin falls behind its latest soaked upstream release). The
// upstream of a perry binding is an npm dist rather than a vendored git
// tree, so the pin lives as toml fields instead of a .gitmodules block,
// but the records and rules are the same:
//
//   [bindings.<name>.upstream]
//   version   = "1.2.3"    # pinned npm release (immutable dist)
//   sha256    = "<64hex>"  # sha256 of the registry tarball at pin time
//   repo      = "<url>"    # upstream source repo, when declared
//   ref       = "<40hex>"  # gitHead recorded by the publisher, when known
//   ported-at = "1.2.3"    # release the wrapper was last REVIEWED against
//   date      = "YYYY-MM-DD"
//
// THE LOCK-STEP RULE: ported-at must equal version. Re-pinning an
// upstream without re-reviewing the wrapper against the upstream diff
// reds the --check gate (and the parser inside perry itself) until
// ported-at advances with the review. An upstream release can never go
// silently stale; a pin bump can never outrun its port.
//
// Modes:
//   --set <name> [version]        provision/update one pin (default: latest)
//   --backfill                    provision every unpinned binding at latest
//   --check                       offline gate: pins present, lock-stepped,
//                                 crates exist. Exit 1 on violation. CI-safe.
//   --check --refresh [--soak-days N]
//                                 network advisory: exit 1 when a pinned
//                                 upstream has a newer stable release that
//                                 has soaked >= N days (default 7)
//   --materialize <name>          shallow-clone the upstream repo at the
//                                 pinned ref into gitignored upstream/<name>
//                                 for port review (diff old pin vs new tag)
//
// Never hand-edit `version`/`sha256`/`ref` — the tarball hash cannot be
// recomputed at edit time. Use --set.

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TOML_PATH = path.join(ROOT, 'crates', 'perry', 'well_known_bindings.toml');
const REGISTRY = 'https://registry.npmjs.org';
const DEFAULT_SOAK_DAYS = 7;
const REGISTRY_TIMEOUT_MS = 30_000;
const TARBALL_TIMEOUT_MS = 60_000;
const GIT_TIMEOUT_MS = 60_000;

// Perry's own packages have no third-party upstream to pin.
const SELF_OWNED = (name) => name.startsWith('@perryts/') || name.startsWith('perry/');

// A binding is exempt from carrying its own npm provenance pin when it is
// perry-owned, a Node builtin (upstream is Node core, not an npm dist), or an
// alias/subpath of another binding (it shares that binding's pin).
const isExempt = (b) =>
  SELF_OWNED(b.name) || b.fields['node-builtin'] === 'true' || Boolean(b.fields['alias-of']);

// ---------------------------------------------------------------------------
// Minimal structural toml handling. The bindings file is machine-managed and
// regular ([bindings.<name>] blocks with flat string fields plus an optional
// [bindings.<name>.upstream] sub-block), so we parse/rewrite it structurally
// instead of pulling a toml dependency into the repo's script surface.
// ---------------------------------------------------------------------------

function parseBindings(raw) {
  const bindings = new Map();
  let current = null;
  let section = null; // 'binding' | 'upstream'
  for (const line of raw.split('\n')) {
    const header = line.match(/^\[bindings\.(?:"([^"]+)"|([^.\]"]+))(\.upstream)?\]\s*$/);
    if (header) {
      const name = header[1] ?? header[2];
      if (header[3]) {
        section = 'upstream';
        current = bindings.get(name);
        if (current) current.upstream = {};
      } else {
        section = 'binding';
        current = { name, fields: {}, upstream: null };
        bindings.set(name, current);
      }
      continue;
    }
    if (/^\[/.test(line)) {
      current = null;
      section = null;
      continue;
    }
    // Quoted string values, plus bare booleans (`node-builtin = true`).
    const kv = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]*)"|(true|false))\s*$/);
    if (kv && current) {
      const value = kv[2] ?? kv[3];
      if (section === 'upstream' && current.upstream) {
        current.upstream[kv[1]] = value;
      } else if (section === 'binding') {
        current.fields[kv[1]] = value;
      }
    }
  }
  return bindings;
}

function upstreamBlockText(name, pin) {
  const key = /^[A-Za-z0-9_-]+$/.test(name) ? name : `"${name}"`;
  const lines = [`[bindings.${key}.upstream]`];
  lines.push(`version = "${pin.version}"`);
  lines.push(`sha256 = "${pin.sha256}"`);
  if (pin.repo) lines.push(`repo = "${pin.repo}"`);
  if (pin.ref) lines.push(`ref = "${pin.ref}"`);
  lines.push(`ported-at = "${pin['ported-at']}"`);
  lines.push(`date = "${pin.date}"`);
  return lines.join('\n');
}

// Insert or replace the upstream sub-block directly after the binding's
// own block, preserving every other byte of the file (comments included).
function writePin(raw, name, pin) {
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  // A binding key may appear bare (`date-fns`) or quoted (`"decimal.js"`,
  // and quoted-anyway `"date-fns"`); match whichever the file actually uses
  // rather than assuming from the name's characters.
  const keyPattern = `(?:${esc}|"${esc}")`;
  const headerRe = new RegExp(`^\\[bindings\\.${keyPattern}\\]\\s*$`, 'm');
  const headerMatch = raw.match(headerRe);
  if (!headerMatch) {
    throw new Error(`no [bindings.${name}] block found in ${TOML_PATH}`);
  }
  const upstreamRe = new RegExp(
    `\\n?\\[bindings\\.${keyPattern}\\.upstream\\]\\s*\\n(?:[ \\t]*[A-Za-z0-9_-]+\\s*=\\s*"[^"]*"\\s*\\n?)*`,
  );
  const existing = raw.match(upstreamRe);
  if (existing) {
    return raw.replace(upstreamRe, `\n${upstreamBlockText(name, pin)}\n`);
  }
  // Append after the binding block: from the header, find the next section
  // header (or EOF) and insert before it.
  const start = headerMatch.index;
  const rest = raw.slice(start + headerMatch[0].length);
  const next = rest.search(/\n\[/);
  const insertAt = next === -1 ? raw.length : start + headerMatch[0].length + next;
  return `${raw.slice(0, insertAt)}\n\n${upstreamBlockText(name, pin)}${raw.slice(insertAt)}`;
}

// ---------------------------------------------------------------------------
// npm registry
// ---------------------------------------------------------------------------

async function fetchJson(url) {
  let res;
  try {
    res = await fetch(url, {
      headers: { accept: 'application/json' },
      signal: AbortSignal.timeout(REGISTRY_TIMEOUT_MS),
    });
  } catch (err) {
    throw new Error(`${url}: request failed or timed out after ${REGISTRY_TIMEOUT_MS}ms`, {
      cause: err,
    });
  }
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
}

async function fetchPackument(name) {
  return fetchJson(`${REGISTRY}/${name.replace('/', '%2f')}`);
}

function latestStable(packument) {
  const version = packument['dist-tags']?.latest;
  if (!version) throw new Error(`${packument.name}: no dist-tags.latest`);
  return version;
}

async function sha256OfTarball(url) {
  let res;
  try {
    res = await fetch(url, { signal: AbortSignal.timeout(TARBALL_TIMEOUT_MS) });
  } catch (err) {
    throw new Error(`${url}: download failed or timed out after ${TARBALL_TIMEOUT_MS}ms`, {
      cause: err,
    });
  }
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const bytes = Buffer.from(await res.arrayBuffer());
  return createHash('sha256').update(bytes).digest('hex');
}

function normalizeRepo(repository) {
  const url = typeof repository === 'string' ? repository : repository?.url;
  if (!url) return undefined;
  return url
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/^ssh:\/\/git@/, 'https://')
    .replace(/\.git$/, '');
}

async function provisionPin(name, requestedVersion) {
  const packument = await fetchPackument(name);
  const version = requestedVersion ?? latestStable(packument);
  const manifest = packument.versions?.[version];
  if (!manifest) throw new Error(`${name}@${version}: version not in registry`);
  const sha256 = await sha256OfTarball(manifest.dist.tarball);
  return {
    version,
    sha256,
    repo: normalizeRepo(manifest.repository),
    ref: manifest.gitHead,
    'ported-at': version,
    date: new Date().toISOString().slice(0, 10),
  };
}

// ---------------------------------------------------------------------------
// modes
// ---------------------------------------------------------------------------

async function modeSet(names, requestedVersion) {
  let raw = fs.readFileSync(TOML_PATH, 'utf8');
  const bindings = parseBindings(raw);
  for (const name of names) {
    if (!bindings.has(name)) {
      throw new Error(`no [bindings.${name}] block — add the binding row first`);
    }
    const pin = await provisionPin(name, requestedVersion);
    raw = writePin(raw, name, pin);
    console.log(`pinned ${name}@${pin.version} sha256:${pin.sha256.slice(0, 12)}…`);
  }
  fs.writeFileSync(TOML_PATH, raw);
}

async function modeBackfill() {
  const raw = fs.readFileSync(TOML_PATH, 'utf8');
  const bindings = parseBindings(raw);
  const unpinned = [...bindings.values()].filter((b) => !b.upstream && !isExempt(b));
  if (unpinned.length === 0) {
    console.log('all bindings pinned — nothing to backfill');
    return;
  }
  for (const b of unpinned) {
    await modeSet([b.name]);
  }
}

function checkOffline(bindings) {
  const failures = [];
  for (const b of bindings.values()) {
    // Aliases inherit their target's provenance — verify the target exists
    // and is itself pinned (or exempt), then skip the pin requirement.
    const aliasTarget = b.fields['alias-of'];
    if (aliasTarget) {
      const target = bindings.get(aliasTarget);
      if (!target) {
        failures.push(`${b.name}: alias-of \`${aliasTarget}\`, which is not a known binding`);
      } else if (!target.upstream && !isExempt(target)) {
        failures.push(`${b.name}: alias-of \`${aliasTarget}\`, which has no pin`);
      }
      continue;
    }
    if (isExempt(b)) continue;
    if (!b.upstream) {
      failures.push(`${b.name}: missing [bindings.${b.name}.upstream] pin`);
      continue;
    }
    for (const field of ['version', 'sha256', 'ported-at', 'date']) {
      if (!b.upstream[field]) failures.push(`${b.name}: upstream pin missing \`${field}\``);
    }
    if (
      b.upstream['ported-at'] &&
      b.upstream.version &&
      b.upstream['ported-at'] !== b.upstream.version
    ) {
      failures.push(
        `${b.name}: LOCK-STEP violation — ported-at (${b.upstream['ported-at']}) != ` +
          `version (${b.upstream.version}). Review the wrapper against the upstream ` +
          `diff, then advance ported-at with the review.`,
      );
    }
    if (b.upstream.sha256 && !/^[0-9a-f]{64}$/.test(b.upstream.sha256)) {
      failures.push(`${b.name}: sha256 is not 64 lowercase hex chars`);
    }
    if (b.upstream.date && !/^\d{4}-\d{2}-\d{2}$/.test(b.upstream.date)) {
      failures.push(`${b.name}: date is not YYYY-MM-DD`);
    }
    const crateDir = path.join(ROOT, 'crates', b.fields.crate ?? '');
    if (!b.fields.crate || !fs.existsSync(crateDir)) {
      failures.push(`${b.name}: crate \`${b.fields.crate}\` not found in workspace`);
    }
  }
  return failures;
}

async function checkRefresh(bindings, soakDays) {
  const advisories = [];
  const now = Date.now();
  for (const b of bindings.values()) {
    if (isExempt(b) || !b.upstream?.version) continue;
    let packument;
    try {
      packument = await fetchPackument(b.name);
    } catch (err) {
      advisories.push(`${b.name}: registry lookup failed (${err.message}) — skipping`);
      continue;
    }
    const latest = latestStable(packument);
    if (latest === b.upstream.version) continue;
    const publishedAt = packument.time?.[latest];
    const soakedDays = publishedAt
      ? Math.floor((now - Date.parse(publishedAt)) / 86_400_000)
      : Infinity;
    if (soakedDays >= soakDays) {
      advisories.push(
        `${b.name}: pinned ${b.upstream.version}, latest stable ${latest} ` +
          `(soaked ${soakedDays}d >= ${soakDays}d) — re-pin, re-review, advance ported-at: ` +
          `node scripts/binding_pins.mjs --set ${b.name}`,
      );
    }
  }
  return advisories;
}

function modeMaterialize(name) {
  const bindings = parseBindings(fs.readFileSync(TOML_PATH, 'utf8'));
  const b = bindings.get(name);
  if (!b) throw new Error(`no [bindings.${name}] block`);
  if (!b.upstream?.repo) throw new Error(`${name}: no upstream repo recorded in its pin`);
  const dest = path.join(ROOT, 'upstream', name.replace('/', '__'));
  const ref = b.upstream.ref;
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  if (fs.existsSync(dest)) {
    console.log(`upstream/${name} already materialized — fetching pin`);
  } else {
    execFileSync('git', ['clone', '--filter=blob:none', '--no-checkout', b.upstream.repo, dest], {
      stdio: 'inherit',
      timeout: GIT_TIMEOUT_MS,
    });
  }
  const checkout = ref || `v${b.upstream.version}`;
  try {
    execFileSync('git', ['-C', dest, 'fetch', '--depth', '1', 'origin', checkout], {
      stdio: 'inherit',
      timeout: GIT_TIMEOUT_MS,
    });
    execFileSync('git', ['-C', dest, 'checkout', '--detach', 'FETCH_HEAD'], { stdio: 'inherit' });
  } catch {
    // Publishers without gitHead and without v-prefixed tags: try the bare version tag.
    execFileSync('git', ['-C', dest, 'fetch', '--depth', '1', 'origin', b.upstream.version], {
      stdio: 'inherit',
      timeout: GIT_TIMEOUT_MS,
    });
    execFileSync('git', ['-C', dest, 'checkout', '--detach', 'FETCH_HEAD'], { stdio: 'inherit' });
  }
  console.log(`materialized upstream/${name} at ${checkout} (gitignored, review-only)`);
}

// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const has = (flag) => args.includes(flag);
const valueOf = (flag) => {
  const i = args.indexOf(flag);
  return i !== -1 ? args[i + 1] : undefined;
};

try {
  if (has('--set')) {
    const name = valueOf('--set');
    if (!name) throw new Error('--set requires a binding name');
    const maybeVersion = args[args.indexOf('--set') + 2];
    const version =
      maybeVersion && !maybeVersion.startsWith('--') ? maybeVersion : undefined;
    await modeSet([name], version);
  } else if (has('--backfill')) {
    await modeBackfill();
  } else if (has('--materialize')) {
    const name = valueOf('--materialize');
    if (!name) throw new Error('--materialize requires a binding name');
    modeMaterialize(name);
  } else if (has('--check')) {
    const bindings = parseBindings(fs.readFileSync(TOML_PATH, 'utf8'));
    const failures = checkOffline(bindings);
    for (const f of failures) console.error(`FAIL ${f}`);
    let advisories = [];
    if (has('--refresh')) {
      const soakDays = Number(valueOf('--soak-days') ?? DEFAULT_SOAK_DAYS);
      advisories = await checkRefresh(bindings, soakDays);
      for (const a of advisories) console.error(`STALE ${a}`);
    }
    if (failures.length || advisories.length) process.exit(1);
    console.log(
      `binding pins OK — ${[...bindings.values()].filter((b) => b.upstream).length} pinned, lock-step holds`,
    );
  } else {
    console.error(
      'usage: binding_pins.mjs --set <name> [version] | --backfill | --check [--refresh [--soak-days N]] | --materialize <name>',
    );
    process.exit(2);
  }
} catch (err) {
  console.error(`error: ${err.message}`);
  process.exit(1);
}
