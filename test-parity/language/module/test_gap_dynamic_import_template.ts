// Test: dynamic import() with a template literal whose interpolation is a
// module-level const. The resolver expands `../../../test-files/locale_${lang}.ts` to the
// single concrete path `./locale_es.ts`, and the resolved namespace exposes
// that target's exports.

const lang: "en" | "es" = "es";

async function main(): Promise<void> {
  const m = await import(`../../../test-files/locale_${lang}.ts`);
  console.log(m.hello);
}

main();
