const state: any = globalThis;
async function main() {
  state.__gap_module_identity_count = 0;
  const seen: any[] = [];
  try { await import('./_helpers/module_evaluation_identity/a.mjs'); } catch (e) { seen.push(e); }
  try { await import('./_helpers/module_evaluation_identity/b.mjs'); } catch (e) { seen.push(e); }
  try { await import('./_helpers/module_evaluation_identity/a.mjs'); } catch (e) { seen.push(e); }
  try { await import('./_helpers/module_evaluation_identity/b.mjs'); } catch (e) { seen.push(e); }
  console.log('evaluations', state.__gap_module_identity_count);
  console.log('errors', seen.join(','));
}
main();
