// Regression test for the "String field assignment becomes [object Object]" bug.
//
// When a TypeScript interface has a string field and the variable is initialized
// from a literal (or a factory function) and then a string variable is assigned
// to that field, perry was storing the value as POINTER_TAG NaN-boxed instead of
// STRING_TAG NaN-boxed. Reading the field back returned "[object Object]"
// instead of the assigned string.
//
// Production symptom: perry-hub's API token auth path created a License via a
// factory function and assigned account_id from a UUID variable. dbSaveLicense
// then sent the field to MySQL which rejected it as "Data too long for column
// account_id". The actual value being sent was the License object reference,
// not the string.

interface License {
  key: string;
  account_id: string;
}

function makeLicense(): License {
  return { key: 'k1', account_id: '' };
}

// Case 1: inline object literal
const f: License = { key: 'k1', account_id: '' };
const v1 = 'hello-world';
f.account_id = v1;
console.log('inline literal: typeof=' + typeof f.account_id + ' val=' + f.account_id);

// Case 2: factory function return
const lic = makeLicense();
const accountId = '7f8223bc-b6be-42ee-8551-a2921a581e63';
lic.account_id = accountId;
console.log('factory: typeof=' + typeof lic.account_id + ' val=' + lic.account_id);

// Case 3: re-assignment after first set
lic.account_id = 'second';
console.log('reassign: typeof=' + typeof lic.account_id + ' val=' + lic.account_id);

// Case 4: assignment from a string literal (was already working, regression guard)
const lic2 = makeLicense();
lic2.account_id = 'literal-direct';
console.log('literal: typeof=' + typeof lic2.account_id + ' val=' + lic2.account_id);
