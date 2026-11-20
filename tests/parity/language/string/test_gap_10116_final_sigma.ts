// #10116: context-sensitive default lowercasing, including WTF-8 boundaries.
function lower(input: string): string { return input.toLowerCase(); }
const cases = [
  ["ΑΣ", "ας"],
  ["ΟΔΥΣΣΕΥΣ", "οδυσσευς"],
  ["Σ", "σ"],
  ["ΣΣ", "σς"],
  ["ΑΣΑ", "ασα"],
  ["AΣ! AΣB", "aς! aσb"],
  ["1Σ", "1σ"],
  ["中Σ", "中σ"],
  ["AΣ1", "aς1"],
  ["AΣ中", "aς中"],
  ["A\u{301}Σ", "a\u{301}ς"],
  ["AΣ\u{301}", "aς\u{301}"],
  ["AΣ\u{301}B", "aσ\u{301}b"],
  ["\u{301}Σ", "\u{301}σ"],
  ["A'Σ", "a'ς"],
  ["AΣ'B", "aσ'b"],
  ["AΣ\u{200d}", "aς\u{200d}"],
  ["AΣ\u{200d}B", "aσ\u{200d}b"],
  ["\u{345}Σ", "\u{345}σ"],
  ["A\u{345}Σ", "a\u{345}ς"],
  ["AΣ\u{345}", "aς\u{345}"],
  ["AΣ\u{345}B", "aσ\u{345}b"],
  ["𐐀Σ", "𐐨ς"],
  ["AΣ𐐀", "aσ𐐨"],
  ["😀Σ", "😀σ"],
  ["AΣ😀", "aς😀"],
  ["İΣ", "i\u{307}ς"],
  ["AΣİ", "aσi\u{307}"],
  ["AΣ\ud800ΣA", "aς\ud800σa"],
  ["A\ud800Σ", "a\ud800σ"],
  ["\udfffAΣ", "\udfffaς"],
  ["AΣ\u0301\udfffB", "aς\u0301\udfffb"],
];
for (let i = 0; i < cases.length; i++) {
  const actual = lower(cases[i][0]);
  console.log(i, actual === cases[i][1]);
}
