// DESIGN-p4l4 §2.2: presence tests observe no identity.
// vr-expect: px,py ADMITTED
// Born in the body that tests it (VR-3a: a record crosses no typed call slot
// until the VR-3b signature ABI).
for (const n of [0, 1, 2]) {
  const r = n > 0 ? { px: n, py: n * 2 } : undefined;
  console.log(r === undefined, r !== undefined, r?.px, r ? r.py : -1, typeof r, r == null);
}
