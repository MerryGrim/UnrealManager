import { tr, getLocale } from './i18n';
import { getLanguagePreference, setLanguagePreference, useLanguage } from './i18n';
export function LanguageSettings() {
    useLanguage();
    return <section><span className="eyebrow">{tr("ИНТЕРФЕЙС")}</span><h3>{tr("Язык приложения")}</h3><div className="setting-row"><div><b>{tr("Язык")}</b><span>{tr("По умолчанию используется язык системы. Если он не поддерживается, выбирается английский.")}</span></div><select aria-label={tr("Язык приложения")} value={getLanguagePreference()} onChange={e => setLanguagePreference(e.target.value)}><option value="system">{tr("Как в системе")}</option><option value="ru">{tr("Русский")}</option><option value="en">{tr("English")}</option><option value="de">{tr("Deutsch")}</option><option value="it">{tr("Italiano")}</option><option value="es">{tr("Español")}</option><option value="fr">{tr("Français")}</option><option value="zh">{tr("简体中文")}</option><option value="ja">日本語</option><option value="ko">한국어</option></select></div></section>;
}

