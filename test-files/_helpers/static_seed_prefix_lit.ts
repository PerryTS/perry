// Only imported dynamically, after the scenario grew and dropped a receiver
// through this literal's first two keys: its module init then canonicalizes
// keys the agent's static seed published at agent start.
export function make(v: number) {
  const o = { ssw_p: v, ssw_q: v + 1, ssw_r: v + 2 };
  // A conditional key-add gives the literal in-object slack (born wide), so a
  // duplicate keys list is a static-id refusal, not only a silent miss.
  // @ts-ignore
  if (v > 100) o.ssw_s = v + 3;
  return o;
}
