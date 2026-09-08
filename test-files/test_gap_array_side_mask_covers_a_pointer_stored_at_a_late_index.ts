// #9983: slice/splice must describe each result slot before a later inherited
// getter can throw and leave a custom-species result reachable. Run the Perry
// binary with PERRY_GC_VERIFY_MARK=1; the manual gc() after each caught throw
// makes the existing mask-free array verifier inspect the partially written
// result. The required pre-fix check is an UNENUMERATED report at index 10.
declare function gc(): void;

function forceFullGc(): void {
  if (typeof gc === "function") {
    gc();
  }
}

function array_side_mask_covers_a_pointer_stored_at_a_late_index(
  operation: "slice" | "splice",
): void {
  // Establish a mixed twelve-slot destination whose old description lists
  // index 0 but not index 10. The custom species keeps this exact array
  // reachable even when the source getter aborts the operation.
  const destination: any[] = new Array(12);
  destination[0] = { old: true };
  for (let i = 1; i < 12; i++) {
    destination[i] = i;
  }

  const late = { label: operation + "-late" };
  const source: any[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, late, ,];
  // Perry's array_iteration_is_exotic predicate observes indexed properties
  // on the canonical Array prototype. Put the inherited getter there so the
  // public slice path is forced through its observable-read loop as well as
  // splice's hole lookup. Restore the prior descriptor in finally.
  const previousIndex11 = Object.getOwnPropertyDescriptor(Array.prototype, "11");
  Object.defineProperty(Array.prototype, "11", {
    configurable: true,
    get() {
      throw new Error(operation + "-stop");
    },
  });

  function SpeciesResult(): any[] {
    return destination;
  }
  (source as any).constructor = { [Symbol.species]: SpeciesResult };

  try {
    let caught = "none";
    try {
      if (operation === "slice") {
        source.slice(0, 12);
      } else {
        source.splice(0, 12);
      }
    } catch (error) {
      caught = (error as Error).message;
    }

    // The throw skips the deferred rebuild. Keep destination observably live
    // across the collection, then verify the value after the diagnostic ran.
    forceFullGc();
    console.log(operation + ":" + caught + ":" + destination[10].label);
  } finally {
    if (previousIndex11 === undefined) {
      delete (Array.prototype as any)[11];
    } else {
      Object.defineProperty(Array.prototype, "11", previousIndex11);
    }
  }
}

array_side_mask_covers_a_pointer_stored_at_a_late_index("slice");
array_side_mask_covers_a_pointer_stored_at_a_late_index("splice");
