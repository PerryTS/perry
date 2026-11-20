export function use(h, depth) {
  if (depth > 0) return use(h, depth - 1);
  return h.f();
}
