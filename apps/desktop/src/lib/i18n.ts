import { derived, get, writable } from 'svelte/store';
import english from './locales/en.json' with { type: 'json' };

export type Language = 'zh' | 'en';
export type Parameters = Record<string, string | number>;
type Message = string | { one: string; other: string };
export const locale = writable<Language>('en');
export function resolveLanguage(tag: string | null | undefined): Language {
  return tag?.split(/[-_]/)[0]?.toLowerCase() === 'zh' ? 'zh' : 'en';
}
export const currentLocale = () => (get(locale) === 'zh' ? 'zh-CN' : 'en-US');
export const currentLanguage = () => get(locale);
export function translateFor(language: Language, key: string, values: Parameters = {}): string {
  const entry = language === 'zh' ? key : ((english as Record<string, Message>)[key] ?? key);
  const message =
    typeof entry === 'string'
      ? entry
      : new Intl.PluralRules('en').select(Number(values.count ?? values._0)) === 'one'
        ? entry.one
        : entry.other;
  return message.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.hasOwn(values, name) ? String(values[name]) : match,
  );
}
export const translator = derived(
  locale,
  (language) => (key: string, values?: Parameters) => translateFor(language, key, values),
);
export const translate = (key: string, values?: Parameters) =>
  translateFor(get(locale), key, values);

/** Localize known app errors; do not rewrite user content or raw provider text. */
export function localizeError(error: unknown, language: Language = currentLanguage()): string {
  const value = String(error).replace(/^Error:\s*/, '');
  if (language === 'en' && /\p{Script=Han}/u.test(value) && !(value in english)) {
    return 'The operation failed. Please try again.';
  }
  return translateFor(language, value);
}

export function selectLanguage(tag: string | null | undefined): void {
  const language = resolveLanguage(tag);
  locale.set(language);
  if (typeof document !== 'undefined')
    document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en';
}
