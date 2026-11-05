// parity-env: PERRY_GC_BUDGETED_OLD_RECLAIM=1 PERRY_GC_MAJOR_PACING_FLOOR_MB=1 PERRY_GC_MAJOR_PACING_GROWTH=1
// Latest small array/class birth held ONLY in a local across sliced cycles.
class Birth {
  value: number;
  payload: any;
  constructor(value: number) {
    this.value = value;
    this.payload = [value, value + 1];
    for (let i = 0; i < 3; i++) {
      const scratch = new Uint8Array(8192);
      scratch[0] = i;
    }
  }
}
function run(): void {
  let latestArray: any = [0, "start"];
  let latestObject: any = new Birth(0);
  let pressure: any = new Uint8Array(1);
  let sum = 0;
  for (let i = 1; i <= 4096; i++) {
    latestArray = [i, { value: i + 1 }];
    latestObject = new Birth(i);
    for (let j = 0; j < 12; j++) {
      pressure = new Uint8Array(32768);
      pressure[0] = j;
    }
    sum += latestArray[0] + latestArray[1].value + latestObject.value + latestObject.payload[1];
  }
  // Finish pending work without copying either subject into another root.
  for (let i = 0; i < 4096; i++) {
    pressure = new Uint8Array(32768);
    pressure[0] = i & 255;
  }
  console.log("inline", sum, latestArray[0], latestArray[1].value, latestObject.value, latestObject.payload[1], pressure[0]);
}
run();
