// demonstrates: per-API utility-package snippets shown in
//   docs/src/stdlib/utilities.md
// docs: docs/src/stdlib/utilities.md
// platforms: macos, linux, windows

// Each ANCHOR block below is the exact code that the utilities docs page
// renders inline (via {{#include ... :NAME}}). The whole file is compiled
// and run by the doc-tests harness, so every snippet is a tested artifact —
// if any snippet drifts from the real package API, CI fails.
//
// uuid, nanoid and validator are compiled from their real npm source (the
// native bindings were removed), so they must be installed: the doc-tests job
// runs `npm ci` at the repo root, whose devDependencies list them. lodash /
// dayjs / moment snippets stay `,no-test` on the markdown page.

// ANCHOR: uuid
import { v4 as uuidv4 } from "uuid"

const id = uuidv4()
console.log(id) // e.g., "550e8400-e29b-41d4-a716-446655440000"
// ANCHOR_END: uuid

// ANCHOR: nanoid
import { nanoid } from "nanoid"

const nid = nanoid() // Default 21 chars
console.log(nid)
// ANCHOR_END: nanoid

// ANCHOR: validator
import validator from "validator"

console.log(validator.isEmail("test@example.com"))  // true
console.log(validator.isURL("https://example.com")) // true
console.log(validator.isUUID(id))                   // true
console.log(validator.isEmpty(""))                  // true
// ANCHOR_END: validator
