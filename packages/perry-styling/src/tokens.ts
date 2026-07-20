// tokens.ts — JSON design token reader and validator

import { PerryColor, parseColor } from "./color";

// A resolved palette: key -> PerryColor
export interface ColorPalette {
  [key: string]: PerryColor;
}

// A resolved spacing/radius/fontSize map: key -> number
export interface NumberMap {
  [key: string]: number;
}

// The fully resolved token set, split into light/dark palettes
export interface TokenSet {
  lightColors: ColorPalette;
  darkColors: ColorPalette;
  colorKeys: string[];        // keys in lightColors (canonical, no -dark suffix)
  spacing: NumberMap;
  radius: NumberMap;
  fontSize: NumberMap;
  borderWidth: NumberMap;
}

// Parse a flat JSON token object into a TokenSet.
// Convention: keys ending in "-dark" are the dark-mode variant of the matching base key.
// Missing dark variants fall back to the light value.
export function parseTokens(json: string): TokenSet {
  const raw = JSON.parse(json);

  const lightColors: ColorPalette = {};
  const darkColors: ColorPalette = {};
  const colorKeys: string[] = [];
  const spacing: NumberMap = {};
  const radius: NumberMap = {};
  const fontSize: NumberMap = {};
  const borderWidth: NumberMap = {};

  // Process colors section
  if (raw !== null && typeof raw === "object") {
    const colors = raw["colors"];
    if (colors !== null && typeof colors === "object") {
      // First pass: collect light keys (keys without -dark suffix)
      const allColorKeys: string[] = [];
      const darkKeySet: string[] = [];

      const colorKeyList = Object.keys(colors);
      for (let i = 0; i < colorKeyList.length; i = i + 1) {
        const k = colorKeyList[i];
        if (k.endsWith("-dark")) {
          darkKeySet.push(k);
        } else {
          allColorKeys.push(k);
        }
      }

      // Build light palette from non-dark keys
      for (let i = 0; i < allColorKeys.length; i = i + 1) {
        const k = allColorKeys[i];
        const val = colors[k];
        if (typeof val === "string") {
          lightColors[k] = parseColor(val);
          colorKeys.push(k);
        }
      }

      // Build dark palette: use -dark variant if present, else fall back to light
      for (let i = 0; i < colorKeys.length; i = i + 1) {
        const k = colorKeys[i];
        const darkKey = k + "-dark";
        const darkVal = colors[darkKey];
        if (typeof darkVal === "string") {
          darkColors[k] = parseColor(darkVal);
        } else {
          // No dark variant — same as light
          darkColors[k] = lightColors[k];
        }
      }
    }

    // Process spacing section
    const spacingRaw = raw["spacing"];
    if (spacingRaw !== null && typeof spacingRaw === "object") {
      const keys = Object.keys(spacingRaw);
      for (let i = 0; i < keys.length; i = i + 1) {
        const k = keys[i];
        const v = spacingRaw[k];
        if (typeof v === "number") {
          spacing[k] = v;
        }
      }
    }

    // Process radius section
    const radiusRaw = raw["radius"];
    if (radiusRaw !== null && typeof radiusRaw === "object") {
      const keys = Object.keys(radiusRaw);
      for (let i = 0; i < keys.length; i = i + 1) {
        const k = keys[i];
        const v = radiusRaw[k];
        if (typeof v === "number") {
          radius[k] = v;
        }
      }
    }

    // Process fontSize section
    const fontSizeRaw = raw["fontSize"];
    if (fontSizeRaw !== null && typeof fontSizeRaw === "object") {
      const keys = Object.keys(fontSizeRaw);
      for (let i = 0; i < keys.length; i = i + 1) {
        const k = keys[i];
        const v = fontSizeRaw[k];
        if (typeof v === "number") {
          fontSize[k] = v;
        }
      }
    }

    // Process borderWidth section
    const borderWidthRaw = raw["borderWidth"];
    if (borderWidthRaw !== null && typeof borderWidthRaw === "object") {
      const keys = Object.keys(borderWidthRaw);
      for (let i = 0; i < keys.length; i = i + 1) {
        const k = keys[i];
        const v = borderWidthRaw[k];
        if (typeof v === "number") {
          borderWidth[k] = v;
        }
      }
    }
  }

  return { lightColors, darkColors, colorKeys, spacing, radius, fontSize, borderWidth };
}
