// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_SCHEDULE_SEED=41 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0
// Captured values retain their scope owner across moving collections.
function keepAcrossCalls(seed: string): number {
  const s0 = seed + "-0";
  const s1 = seed + "-1";
  const s2 = seed + "-2";
  const s3 = seed + "-3";
  const s4 = seed + "-4";
  const s5 = seed + "-5";
  const s6 = seed + "-6";
  const s7 = seed + "-7";
  const s8 = seed + "-8";
  const s9 = seed + "-9";
  const s10 = seed + "-10";
  const s11 = seed + "-11";
  const s12 = seed + "-12";
  const s13 = seed + "-13";
  const s14 = seed + "-14";
  const s15 = seed + "-15";
  const s16 = seed + "-16";
  const s17 = seed + "-17";
  const s18 = seed + "-18";
  const s19 = seed + "-19";
  const s20 = seed + "-20";
  const s21 = seed + "-21";
  const s22 = seed + "-22";
  const s23 = seed + "-23";
  const s24 = seed + "-24";
  const s25 = seed + "-25";
  const s26 = seed + "-26";
  const s27 = seed + "-27";
  const s28 = seed + "-28";
  const s29 = seed + "-29";
  const s30 = seed + "-30";
  const s31 = seed + "-31";
  for (let i = 0; i < 100; i++) {
    const trash = { text: seed + i, values: [i, i + 1] };
    if (trash.text.length === 0) throw new Error("empty");
  }
  function readAll(): number {
    return s0.length + s1.length + s2.length + s3.length + s4.length + s5.length + s6.length + s7.length + s8.length + s9.length + s10.length + s11.length + s12.length + s13.length + s14.length + s15.length + s16.length + s17.length + s18.length + s19.length + s20.length + s21.length + s22.length + s23.length + s24.length + s25.length + s26.length + s27.length + s28.length + s29.length + s30.length + s31.length;
  }
  return readAll();
}
console.log(keepAcrossCalls(process.argv[2] || "held"));
