// Shared design-system token loading, resolution, and WCAG contrast (T100).

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

export function loadTokens(root) {
  const path = join(root, 'design-system', 'tokens.json');
  return JSON.parse(readFileSync(path, 'utf8'));
}

function parseHex(hex) {
  const value = hex.replace('#', '');
  if (value.length !== 6) return null;
  const r = parseInt(value.slice(0, 2), 16) / 255;
  const g = parseInt(value.slice(2, 4), 16) / 255;
  const b = parseInt(value.slice(4, 6), 16) / 255;
  return [r, g, b];
}

function linearize(channel) {
  return channel <= 0.04045
    ? channel / 12.92
    : ((channel + 0.055) / 1.055) ** 2.4;
}

function luminance(hex) {
  const [r, g, b] = parseHex(hex);
  if (r === null || g === null || b === null) return 0;
  return (
    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
  );
}

export function contrastRatio(hexA, hexB) {
  const la = luminance(hexA);
  const lb = luminance(hexB);
  const lighter = Math.max(la, lb);
  const darker = Math.min(la, lb);
  return (lighter + 0.05) / (darker + 0.05);
}

// Resolves a theme's semantic color tokens to concrete hex values.
// `theme` is 'semantic' (baseline) or 'high_contrast' (baseline + overrides).
export function resolveTheme(tokens, theme) {
  const semantic = tokens.semantic.color;
  const overrides = theme === 'high_contrast' ? tokens.high_contrast?.color ?? {} : {};
  const primitives = tokens.primitives.color;
  const resolved = {};
  for (const [key, reference] of Object.entries(semantic)) {
    const effective = Object.prototype.hasOwnProperty.call(overrides, key)
      ? overrides[key]
      : reference;
    resolved[key] = primitives[effective] ?? null;
  }
  // High-contrast-only extensions (e.g. focus.ring) resolve too.
  for (const [key, reference] of Object.entries(overrides)) {
    if (!Object.prototype.hasOwnProperty.call(semantic, key)) {
      resolved[key] = primitives[reference] ?? null;
    }
  }
  return resolved;
}

export function lint(tokens) {
  const errors = [];
  const required = ['primitives', 'semantic', 'high_contrast', 'contrast_pairs', 'min_contrast_ratio'];
  for (const key of required) {
    if (!(key in tokens)) errors.push(`tokens.json is missing "${key}"`);
  }
  if (tokens.schema_version !== 1) errors.push('schema_version must be 1');
  if (!tokens.primitives || !tokens.semantic) return errors;

  for (const family of ['color', 'typography', 'spacing', 'radius', 'breakpoint']) {
    if (!(family in tokens.primitives)) errors.push(`primitives is missing "${family}"`);
  }
  for (const family of ['color', 'typography', 'spacing', 'radius']) {
    if (!(family in tokens.semantic)) errors.push(`semantic is missing "${family}"`);
  }

  const primitives = tokens.primitives.color ?? {};
  const semantic = tokens.semantic.color ?? {};
  const overrides = tokens.high_contrast?.color ?? {};

  for (const [key, reference] of Object.entries(semantic)) {
    if (!(reference in primitives)) {
      errors.push(`semantic.color.${key} references unknown primitive "${reference}"`);
    }
  }
  for (const [key, reference] of Object.entries(overrides)) {
    if (!(reference in primitives)) {
      errors.push(`high_contrast.color.${key} references unknown primitive "${reference}"`);
    }
  }

  const breakpoints = tokens.primitives.breakpoint ?? {};
  const values = Object.entries(breakpoints)
    .filter(([, value]) => typeof value === 'number')
    .map(([, value]) => value);
  for (let index = 1; index < values.length; index += 1) {
    if (values[index] <= values[index - 1]) {
      errors.push('breakpoints must be strictly increasing');
      break;
    }
  }

  const minRatio = tokens.min_contrast_ratio ?? 4.5;
  const baseline = resolveTheme(tokens, 'semantic');
  const highContrast = resolveTheme(tokens, 'high_contrast');
  for (const [foreground, background] of tokens.contrast_pairs ?? []) {
    for (const [name, theme] of [['baseline', baseline], ['high-contrast', highContrast]]) {
      const fg = theme[foreground];
      const bg = theme[background];
      if (!fg || !bg) {
        errors.push(`${name} theme is missing contrast pair ${foreground}/${background}`);
        continue;
      }
      const ratio = contrastRatio(fg, bg);
      if (ratio < minRatio) {
        errors.push(
          `${name} theme ${foreground} on ${background} contrast ${ratio.toFixed(2)} < ${minRatio}`,
        );
      }
    }
  }
  return errors;
}

// Deterministic snapshot: resolved tokens as a sorted-key object.
export function snapshot(tokens, theme) {
  const resolved = resolveTheme(tokens, theme);
  const sorted = {};
  for (const key of Object.keys(resolved).sort()) sorted[key] = resolved[key];
  return { schema_version: 1, theme, resolved: sorted };
}