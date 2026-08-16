export interface ThemeInfo {
  id: string;
  name: string;
  accent: string;
  background: string;
  foreground: string;
  terminal?: unknown;
}

export interface ThemeListResult {
  themes: ThemeInfo[];
  active: string;
}

export interface ThemeApplyResult {
  active: string;
}

export interface ThemeAccentResult {
  accent: string;
}

export interface WindowAlphaResult {
  alpha: number;
}