/**
 * @file The 🖐 HUMAN GATE block, verbatim shape from the global CLAUDE.md
 *   human-gate prompt protocol. The publish approve stage renders this when
 *   it reaches a step only the human can clear (the browser web-OTP 2FA
 *   promote). Pure — returns the block string; the caller prints it.
 *
 *   Lane A is copy-pasteable (the verbatim command the human runs); lane B is
 *   what an agent says to drive the same command. Both lanes always present.
 */

export interface HumanGateSpec {
  /** Short label, e.g. "approve". */
  name: string
  /** Index/total, e.g. "1/1". */
  index?: string
  /** What is blocked and why, one sentence. */
  need: string
  /** The active guard/tool restriction that shaped the lanes. */
  mind: string
  /** Lane A: the verbatim phrase to type, or the exact `! <command>` to run. */
  you: string
  /** Lane B: what the agent says to drive the SAME command. */
  me: string
  /** What resumes once cleared. */
  then: string
}

/**
 * Render the 🖐 HUMAN GATE block. Kept byte-faithful to the global CLAUDE.md
 * shape: a space between the glyph and the label, lanes A/B always present.
 */
export function formatHumanGate(spec: HumanGateSpec): string {
  const idx = spec.index ? ` [${spec.index}]` : ''
  return [
    `🖐  HUMAN GATE — ${spec.name}${idx}`,
    `  Need: ${spec.need}`,
    `  Mind: ${spec.mind}`,
    `  A) You: ${spec.you}`,
    `  B) Me: ${spec.me}`,
    `  Then: ${spec.then}`,
  ].join('\n')
}

/** The approve gate the publish pipeline raises at the promote step. */
export function formatApproveGate(opts: {
  version: string
  repoPath: string
}): string {
  return formatHumanGate({
    name: 'approve',
    index: '1/1',
    need: `the staged @perryts/* v${opts.version} upload is verified + socket-scanned; the 2FA promote to live is a human action.`,
    mind: 'npm stage approve requires browser web-OTP 2FA — no long-lived token; the stage upload ran in CI under OIDC.',
    you: `cd ${opts.repoPath} && npm run publish:approve`,
    me: 'I will run `npm run publish:approve` so npm opens the browser for web-OTP 2FA and promotes the staged entries to live.',
    then: 'registry liveness is confirmed, the vX.Y.Z tag + immutable GitHub release are cut (draft→upload install.sh+checksums+tarballs→undraft), and the brew/apt/cargo legs run.',
  })
}
