import { invoke } from '@tauri-apps/api/core';
import { selectLanguage } from './i18n';

let pending: Promise<void> | null = null;
export function refreshLanguage(): Promise<void> {
  if (pending) return pending;
  pending = (async () => {
    let language = navigator.language;
    if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'readme') {
      language = new URLSearchParams(location.search).get('language') ?? language;
    }
    if ('__TAURI_INTERNALS__' in window) {
      try {
        language = await invoke<string>('get_system_language');
      } catch {
        language = 'en';
      }
    }
    selectLanguage(language);
  })().finally(() => {
    pending = null;
  });
  return pending;
}
