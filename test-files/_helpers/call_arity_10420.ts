// Helper module for test_gap_10420_call_arity_limits.ts (#10420): function
// VALUES exported across a module boundary -- an arrow `const`, a qs-shaped
// named function expression bound to a `var`, and a rest arrow.

export function sig(values: any[]): string {
  let sum = 0;
  for (let i = 0; i < values.length; i++) sum += values[i];
  return `${values.length}:${values[0]}:${values[values.length - 1]}:${sum}`;
}

export const importedArrow3 = (
  p0: any, p1: any, p2: any,
): string =>
  sig([
    p0, p1, p2,
  ]);

export var importedFexpr3 = function importedFexpr3(
  p0: any, p1: any, p2: any,
): string {
  return sig([
    p0, p1, p2,
  ]);
};

export const importedArrow16 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
): string =>
  sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
  ]);

export var importedFexpr16 = function importedFexpr16(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
  ]);
};

export const importedArrow17 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
): string =>
  sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]);

export var importedFexpr17 = function importedFexpr17(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]);
};

export const importedArrow18 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any,
): string =>
  sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
  ]);

export var importedFexpr18 = function importedFexpr18(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any,
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
  ]);
};

export const importedArrow32 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any,
): string =>
  sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
  ]);

export var importedFexpr32 = function importedFexpr32(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any,
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
  ]);
};

export const importedArrow64 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any, p35: any, p36: any,
  p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any, p44: any, p45: any,
  p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any, p53: any, p54: any,
  p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
): string =>
  sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
    p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
    p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
  ]);

export var importedFexpr64 = function importedFexpr64(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any, p35: any, p36: any,
  p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any, p44: any, p45: any,
  p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any, p53: any, p54: any,
  p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
    p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
    p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
  ]);
};

export const importedRest = (...values: any[]): string => sig(values);
