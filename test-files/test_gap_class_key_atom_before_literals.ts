// A class key list built before the module holding its key literals runs
// (zod imported next to marked, release blocker B12). The importer builds the
// imported class's key list in its own string init, which runs before the
// defining module registers its literals; when `alias` is not one of the
// program's coloured keys, the runtime minted a second atom for it. The
// object's keys held that atom, the defining module's read site passed its
// image literal, the probe answered absent, and `this.alias.bind(this)` threw
// "Bind must be called on a function".
import { StringSchema } from "./_helpers/class_key_atom/schema.ts";

// More than the colour budget (127 keys) of keys read more often than
// `alias` (read once, in the constructor), so `alias` gets no colour, as
// under a large second package.
const wide = {
  w000: 0, w001: 1, w002: 2, w003: 3, w004: 4, w005: 5, w006: 6, w007: 7, w008: 8, w009: 9,
  w010: 10, w011: 11, w012: 12, w013: 13, w014: 14, w015: 15, w016: 16, w017: 17, w018: 18, w019: 19,
  w020: 20, w021: 21, w022: 22, w023: 23, w024: 24, w025: 25, w026: 26, w027: 27, w028: 28, w029: 29,
  w030: 30, w031: 31, w032: 32, w033: 33, w034: 34, w035: 35, w036: 36, w037: 37, w038: 38, w039: 39,
  w040: 40, w041: 41, w042: 42, w043: 43, w044: 44, w045: 45, w046: 46, w047: 47, w048: 48, w049: 49,
  w050: 50, w051: 51, w052: 52, w053: 53, w054: 54, w055: 55, w056: 56, w057: 57, w058: 58, w059: 59,
  w060: 60, w061: 61, w062: 62, w063: 63, w064: 64, w065: 65, w066: 66, w067: 67, w068: 68, w069: 69,
  w070: 70, w071: 71, w072: 72, w073: 73, w074: 74, w075: 75, w076: 76, w077: 77, w078: 78, w079: 79,
  w080: 80, w081: 81, w082: 82, w083: 83, w084: 84, w085: 85, w086: 86, w087: 87, w088: 88, w089: 89,
  w090: 90, w091: 91, w092: 92, w093: 93, w094: 94, w095: 95, w096: 96, w097: 97, w098: 98, w099: 99,
  w100: 100, w101: 101, w102: 102, w103: 103, w104: 104, w105: 105, w106: 106, w107: 107, w108: 108, w109: 109,
  w110: 110, w111: 111, w112: 112, w113: 113, w114: 114, w115: 115, w116: 116, w117: 117, w118: 118, w119: 119,
  w120: 120, w121: 121, w122: 122, w123: 123, w124: 124, w125: 125, w126: 126, w127: 127, w128: 128, w129: 129,
};
function sum(o: typeof wide): number {
  return (
    o.w000 + o.w001 + o.w002 + o.w003 + o.w004 + o.w005 + o.w006 + o.w007 + o.w008 + o.w009 +
    o.w010 + o.w011 + o.w012 + o.w013 + o.w014 + o.w015 + o.w016 + o.w017 + o.w018 + o.w019 +
    o.w020 + o.w021 + o.w022 + o.w023 + o.w024 + o.w025 + o.w026 + o.w027 + o.w028 + o.w029 +
    o.w030 + o.w031 + o.w032 + o.w033 + o.w034 + o.w035 + o.w036 + o.w037 + o.w038 + o.w039 +
    o.w040 + o.w041 + o.w042 + o.w043 + o.w044 + o.w045 + o.w046 + o.w047 + o.w048 + o.w049 +
    o.w050 + o.w051 + o.w052 + o.w053 + o.w054 + o.w055 + o.w056 + o.w057 + o.w058 + o.w059 +
    o.w060 + o.w061 + o.w062 + o.w063 + o.w064 + o.w065 + o.w066 + o.w067 + o.w068 + o.w069 +
    o.w070 + o.w071 + o.w072 + o.w073 + o.w074 + o.w075 + o.w076 + o.w077 + o.w078 + o.w079 +
    o.w080 + o.w081 + o.w082 + o.w083 + o.w084 + o.w085 + o.w086 + o.w087 + o.w088 + o.w089 +
    o.w090 + o.w091 + o.w092 + o.w093 + o.w094 + o.w095 + o.w096 + o.w097 + o.w098 + o.w099 +
    o.w100 + o.w101 + o.w102 + o.w103 + o.w104 + o.w105 + o.w106 + o.w107 + o.w108 + o.w109 +
    o.w110 + o.w111 + o.w112 + o.w113 + o.w114 + o.w115 + o.w116 + o.w117 + o.w118 + o.w119 +
    o.w120 + o.w121 + o.w122 + o.w123 + o.w124 + o.w125 + o.w126 + o.w127 + o.w128 + o.w129 +
    0
  );
}
function sumAgain(o: typeof wide): number {
  return (
    o.w000 + o.w001 + o.w002 + o.w003 + o.w004 + o.w005 + o.w006 + o.w007 + o.w008 + o.w009 +
    o.w010 + o.w011 + o.w012 + o.w013 + o.w014 + o.w015 + o.w016 + o.w017 + o.w018 + o.w019 +
    o.w020 + o.w021 + o.w022 + o.w023 + o.w024 + o.w025 + o.w026 + o.w027 + o.w028 + o.w029 +
    o.w030 + o.w031 + o.w032 + o.w033 + o.w034 + o.w035 + o.w036 + o.w037 + o.w038 + o.w039 +
    o.w040 + o.w041 + o.w042 + o.w043 + o.w044 + o.w045 + o.w046 + o.w047 + o.w048 + o.w049 +
    o.w050 + o.w051 + o.w052 + o.w053 + o.w054 + o.w055 + o.w056 + o.w057 + o.w058 + o.w059 +
    o.w060 + o.w061 + o.w062 + o.w063 + o.w064 + o.w065 + o.w066 + o.w067 + o.w068 + o.w069 +
    o.w070 + o.w071 + o.w072 + o.w073 + o.w074 + o.w075 + o.w076 + o.w077 + o.w078 + o.w079 +
    o.w080 + o.w081 + o.w082 + o.w083 + o.w084 + o.w085 + o.w086 + o.w087 + o.w088 + o.w089 +
    o.w090 + o.w091 + o.w092 + o.w093 + o.w094 + o.w095 + o.w096 + o.w097 + o.w098 + o.w099 +
    o.w100 + o.w101 + o.w102 + o.w103 + o.w104 + o.w105 + o.w106 + o.w107 + o.w108 + o.w109 +
    o.w110 + o.w111 + o.w112 + o.w113 + o.w114 + o.w115 + o.w116 + o.w117 + o.w118 + o.w119 +
    o.w120 + o.w121 + o.w122 + o.w123 + o.w124 + o.w125 + o.w126 + o.w127 + o.w128 + o.w129 +
    0
  );
}
console.log("wide", sum(wide), sumAgain(wide));

const s = StringSchema.create();
const aliasKey = ["ali", "as"].join("");
const alias = (s as any)[aliasKey];
console.log(typeof alias, alias("x"), s.check(1));
console.log(Object.keys(s).join(","));
