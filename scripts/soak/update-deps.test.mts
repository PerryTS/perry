import assert from 'node:assert/strict'
import { test } from 'node:test'

import { isMinPublishAgeUnsupported, selectEcosystems } from './update-deps.mts'

test('no ecosystem flag updates both', () => {
  assert.deepEqual(selectEcosystems([]), { npm: true, cargo: true })
  assert.deepEqual(selectEcosystems(['--dry-run']), { npm: true, cargo: true })
})

test('a single flag selects only that ecosystem', () => {
  assert.deepEqual(selectEcosystems(['--npm']), { npm: true, cargo: false })
  assert.deepEqual(selectEcosystems(['--cargo']), { npm: false, cargo: true })
})

// Regression: a naive "flag present = only that one" reading made
// `--npm --cargo` run NEITHER ecosystem.
test('naming both flags updates both', () => {
  assert.deepEqual(selectEcosystems(['--npm', '--cargo']), { npm: true, cargo: true })
})

// The cargo soak is a warning-only unused key on any cargo that does not
// implement it, so this string is the ONLY evidence it silently did not
// apply. Pin the exact wording cargo emits.
test('detects the unused-config-key warning that means the cargo soak did not apply', () => {
  const real =
    'warning: unused config key `unstable.min-publish-age` in `/repo/.cargo/config.toml`\n'
  assert.equal(isMinPublishAgeUnsupported(real), true)
  assert.equal(isMinPublishAgeUnsupported('warning: unused config key `unstable.other`\n'), false)
  assert.equal(isMinPublishAgeUnsupported(''), false)
  assert.equal(isMinPublishAgeUnsupported('    Updating crates.io index\n'), false)
})
