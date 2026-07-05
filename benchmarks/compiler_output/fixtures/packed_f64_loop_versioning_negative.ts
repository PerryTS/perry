function holeyArrayLoop(): number {
  const values: number[] = new Array(3);
  values[1] = 4;
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    const value: any = (values as any)[i];
    sum += typeof value === "undefined" ? 10 : value;
  }
  return sum;
}

function sparseWriteLoop(): number {
  const values: number[] = [1, 2];
  (values as any)[4] = 9;
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    const value: any = (values as any)[i];
    sum += typeof value === "undefined" ? 10 : value;
  }
  return sum;
}

function frozenLoop(): number {
  const values: number[] = [3, 4];
  Object.freeze(values);
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
  }
  return sum;
}

function sealedLoop(): number {
  const values: number[] = [5, 6];
  Object.seal(values);
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
  }
  return sum;
}

function nonExtensibleLoop(): number {
  const values: number[] = [7, 8];
  Object.preventExtensions(values);
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
  }
  return sum;
}

function indexAccessor(): number {
  return 11;
}

function accessorDescriptorLoop(): number {
  const values: any = [1, 2];
  Object.defineProperty(values, "0", {
    get: indexAccessor,
    enumerable: true,
    configurable: true,
  });
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
  }
  return sum;
}

function nonNumberWriteLoop(): number {
  const values: number[] = [1, 2, 3];
  (values as any)[1] = "x";
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    const value: any = (values as any)[i];
    sum += typeof value === "string" ? 5 : value;
  }
  return sum;
}

function anyReceiverLoop(values: any): number {
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
  }
  return sum;
}

function unknownIndexLoop(): number {
  const values: number[] = [4, 5, 6];
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    const index: any = i;
    sum += values[index];
  }
  return sum;
}

function aliasLengthMutationLoop(): number {
  const values: number[] = [1, 2, 3];
  const alias = values;
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    if (i === 0) {
      alias.push(4);
    }
    sum += values[i];
  }
  return sum;
}

function packedF64LoopVersioningNegativeChecksum(): number {
  return (
    holeyArrayLoop() +
    sparseWriteLoop() +
    frozenLoop() +
    sealedLoop() +
    nonExtensibleLoop() +
    accessorDescriptorLoop() +
    nonNumberWriteLoop() +
    anyReceiverLoop([2, 3, 4]) +
    unknownIndexLoop() +
    aliasLengthMutationLoop()
  );
}

console.log(packedF64LoopVersioningNegativeChecksum());
