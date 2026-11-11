declare function gc(): void;

function forceGc(): void {
  if (typeof gc === "function") {
    gc();
  }
}

const nums: number[] = [];
for (let i = 0; i < 128; i++) {
  nums[i] = i + 0.5;
}

forceGc();

let sum = 0;
for (let i = 0; i < nums.length; i++) {
  sum += nums[i];
}

const mixed = nums as any[];
mixed[17] = { label: "heap-value", seen: sum };
forceGc();

console.log(mixed[17].label + ":" + mixed.length + ":" + Math.round(sum));
