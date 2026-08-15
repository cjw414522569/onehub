// Shared i18n loading, linting, pseudo-localization, and snapshots (T118).

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

export function loadLocale(root, locale) {
  const path = join(root, 'i18n', `messages.${locale}.json`);
  return JSON.parse(readFileSync(path, 'utf8'));
}

// Pseudo-localization: prefix/suffix + ASCII -> accented expansion to catch
// truncation and hard-coded text.
const PSEUDO_MAP = {
  a: 'ȧ', b: 'ƀ', c: 'ƈ', d: 'ḋ', e: 'ė', f: 'ƒ', g: 'ǧ', h: 'ħ', i: 'į', j: 'ĵ',
  k: 'ǩ', l: 'ŀ', m: 'ṃ', n: 'ń', o: 'ȯ', p: 'ƥ', q: 'ɋ', r: 'ř', s: 'ş', t: 'ŧ',
  u: 'ų', v: 'ṿ', w: 'ẇ', x: 'ẋ', y: 'ỵ', z: 'ẓ', A: 'Ȧ', B: 'Ɓ', C: 'Ƈ', D: 'Ḋ',
  E: 'Ė', F: 'Ƒ', G: 'Ǧ', H: 'Ħ', I: 'Į', J: 'Ĵ', K: 'Ǩ', L: 'Ŀ', M: 'Ṃ', N: 'Ń',
  O: 'Ȯ', P: 'Ƥ', Q: 'Ɋ', R: 'Ř', S: 'Ş', T: 'Ŧ', U: 'Ų', V: 'Ṿ', W: 'Ẇ', X: 'Ẋ',
  Y: 'Ỵ', Z: 'Ẓ', '0': '0', '1': '1', '2': '2', '3': '3', '4': '4', '5': '5',
  '6': '6', '7': '7', '8': '8', '9': '9',
};

export function pseudoLocalize(text) {
  let out = '⟦';
  for (const character of text) {
    out += PSEUDO_MAP[character] ?? character;
  }
  out += '⟧';
  return out;
}

export function placeholders(text) {
  const found = [];
  const pattern = /\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g;
  let match;
  while ((match = pattern.exec(text)) !== null) found.push(match[1]);
  return found.sort();
}

const KEY_PATTERN = /^[a-z][a-z0-9]*(\.[a-z][a-z0-9]*)*$/;

export function lint(root) {
  const errors = [];
  const en = loadLocale(root, 'en');
  const zh = loadLocale(root, 'zh-CN');

  for (const [locale, resource] of [['en', en], ['zh-CN', zh]]) {
    if (resource.schema_version !== 1) errors.push(`${locale}: schema_version must be 1`);
    if (!resource.messages || typeof resource.messages !== 'object') {
      errors.push(`${locale}: missing messages`);
      continue;
    }
    const max = resource.max_message_chars ?? 120;
    for (const [key, value] of Object.entries(resource.messages)) {
      if (!KEY_PATTERN.test(key)) errors.push(`${locale}: bad message id "${key}"`);
      if (typeof value !== 'string' || value.length === 0) {
        errors.push(`${locale}: empty message for "${key}"`);
      } else if (value.length > max) {
        errors.push(`${locale}: "${key}" exceeds ${max} chars (truncation risk)`);
      } else if (pseudoLocalize(value).length > max * 1.5) {
        errors.push(`${locale}: "${key}" pseudo-localized length risks truncation`);
      }
    }
    if (!resource.plurals?.category?.length) errors.push(`${locale}: missing plural categories`);
    if (!resource.dates?.['format.short'] || !resource.dates?.['format.long'] || !resource.dates?.['time.hms']) {
      errors.push(`${locale}: missing date/time formats`);
    }
    if (!resource.rtl?.supported || !resource.rtl?.mirrored_layouts?.length || !resource.rtl?.locale_rtl?.length) {
      errors.push(`${locale}: missing RTL layout basics`);
    }
  }

  // Shared message IDs: identical key sets.
  const enKeys = Object.keys(en.messages ?? {}).sort();
  const zhKeys = Object.keys(zh.messages ?? {}).sort();
  if (enKeys.join('|') !== zhKeys.join('|')) {
    errors.push('locales do not share the same message IDs');
  }

  // Placeholders must match between locales for every shared key.
  for (const key of enKeys) {
    const enPlaceholders = placeholders(en.messages[key]).join(',');
    const zhPlaceholders = placeholders(zh.messages[key]).join(',');
    if (enPlaceholders !== zhPlaceholders) {
      errors.push(`placeholder mismatch for "${key}": en[${enPlaceholders}] vs zh-CN[${zhPlaceholders}]`);
    }
  }

  // Plural keys: every `.one`/`.other` pair exists in both locales.
  for (const key of enKeys) {
    if (key.endsWith('.one')) {
      const other = key.slice(0, -'.one'.length) + '.other';
      if (!enKeys.includes(other)) errors.push(`en: plural pair missing for "${key}"`);
      if (!zhKeys.includes(other)) errors.push(`zh-CN: plural pair missing for "${other}"`);
    }
  }

  return errors;
}

export function snapshot(root, locale) {
  const resource = loadLocale(root, locale);
  const messages = {};
  for (const key of Object.keys(resource.messages).sort()) {
    messages[key] = resource.messages[key];
  }
  return {
    schema_version: 1,
    locale,
    messages,
    plural_categories: resource.plurals.category,
    date_formats: resource.dates,
    rtl: resource.rtl,
  };
}