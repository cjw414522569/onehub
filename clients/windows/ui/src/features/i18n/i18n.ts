import { useCallback, useEffect, useState } from "react";
import { i18nGetLanguage, i18nSetLanguage } from "../../shared/tauri/commands";

// Minimal chrome dictionaries for the language switcher (T053). The backend
// validates the full resource sets; the UI applies these to key toolbar
// labels so switching Simplified / Traditional / English is visible.
type LangCode = "zh-CN" | "zh-TW" | "en";

const DICTIONARIES: Record<LangCode, Record<string, string>> = {
  "zh-CN": {
    new_connection: "新建连接",
    refresh: "刷新连接并探测延迟",
    database: "数据库",
    redis: "Redis",
    mongo: "Mongo",
    notes: "笔记",
    broadcast: "广播",
    copy_across: "跨服复制",
    agent: "Agent",
    git: "Git",
    acp: "ACP",
    extensions: "扩展",
    language_label: "语言",
  },
  "zh-TW": {
    new_connection: "新建連線",
    refresh: "重新整理連線並探測延遲",
    database: "資料庫",
    redis: "Redis",
    mongo: "Mongo",
    notes: "筆記",
    broadcast: "廣播",
    copy_across: "跨伺服器複製",
    agent: "Agent",
    git: "Git",
    acp: "ACP",
    extensions: "擴充",
    language_label: "語言",
  },
  en: {
    new_connection: "New Connection",
    refresh: "Refresh connections",
    database: "Database",
    redis: "Redis",
    mongo: "Mongo",
    notes: "Notes",
    broadcast: "Broadcast",
    copy_across: "Copy Across",
    agent: "Agent",
    git: "Git",
    acp: "ACP",
    extensions: "Extensions",
    language_label: "Language",
  },
};

const LANGS: { code: LangCode; label: string }[] = [
  { code: "zh-CN", label: "简体中文" },
  { code: "zh-TW", label: "繁體中文" },
  { code: "en", label: "English" },
];

export function useI18n() {
  const [language, setLanguage] = useState<LangCode>("zh-CN");

  useEffect(() => {
    void i18nGetLanguage()
      .then((result) => {
        if (isLangCode(result.language)) {
          setLanguage(result.language);
        }
      })
      .catch(() => undefined);
  }, []);

  const setLanguageAndPersist = useCallback(async (next: LangCode) => {
    setLanguage(next);
    try {
      await i18nSetLanguage(next);
    } catch {
      // bridge unavailable (browser preview)
    }
  }, []);

  const t = useCallback(
    (key: string) => DICTIONARIES[language][key] ?? key,
    [language],
  );

  return { language, setLanguageAndPersist, t, languages: LANGS };
}

function isLangCode(value: string): value is LangCode {
  return value === "zh-CN" || value === "zh-TW" || value === "en";
}

export function validateDictionaries(): { language: LangCode; keys: number }[] {
  return LANGS.map(({ code }) => ({ language: code, keys: Object.keys(DICTIONARIES[code]).length }));
}