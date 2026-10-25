const loaded = (function() {
  try { var imported = { value: 123 }; } catch (unused) {}
  return { get: () => imported.value };
})();
console.log(loaded.get());
