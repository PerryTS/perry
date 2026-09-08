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
  const sourcePrototype = Object.create(Array.prototype);
  Object.defineProperty(sourcePrototype, "11", {
    configurable: true,
    get() {
      throw new Error(operation + "-stop");
    },
  });
  Object.setPrototypeOf(source, sourcePrototype);

  function SpeciesResult(): any[] {
    return destination;
  }
  (source as any).constructor = { [Symbol.species]: SpeciesResult };

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

  // The throw skips the deferred rebuild. The value itself is observable;
  // the diagnostic must also find that the collector walk lists its slot.
  console.log(operation + ":" + caught + ":" + destination[10].label);
  forceFullGc();
}

array_side_mask_covers_a_pointer_stored_at_a_late_index("slice");
array_side_mask_covers_a_pointer_stored_at_a_late_index("splice");
