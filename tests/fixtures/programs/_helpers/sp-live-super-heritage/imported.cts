class ImportedBase { constructor(value) { this.value = value; } }
class ImportedFirst extends ImportedBase { first = 1; }
class ImportedSecond extends ImportedBase { second = 2; }
module.exports = { ImportedFirst, ImportedSecond };
