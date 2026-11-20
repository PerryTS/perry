// Helper for test_gap_10463_entry_block_allocas.ts: the date-fns 4.4.0
// `addMinutes` shape (`addMinutes.js:31-34`). Imported, so that the
// cross-module inliner copies its `setTime` call into the caller's loop.
export function addMinutes(date: Date | number, amount: number): Date {
  const _date = new Date(date instanceof Date ? date.getTime() : date);
  if (isNaN(amount)) return new Date(NaN);
  _date.setTime(_date.getTime() + amount * 60_000);
  return _date;
}
