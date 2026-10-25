`instanceof` against a non-class right operand now stops at a self link or prototype cycle instead of walking 100,000 hops (about 130 ms for `fsReadStream instanceof Error`).
