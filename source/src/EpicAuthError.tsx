import {useEffect} from 'react';
import {tr} from './i18n';

export const EPIC_LOGIN_EVENT = 'um-epic-login';
export function isEpicAuthRequired(error: unknown): boolean {
    return (error instanceof Error ? error.message : error) === 'UM_EPIC_AUTH_REQUIRED';
}
export function useEpicAuthLogin(openLogin: () => void) {
    useEffect(() => {
        const login = () => openLogin();
        window.addEventListener(EPIC_LOGIN_EVENT, login);
        return () => window.removeEventListener(EPIC_LOGIN_EVENT, login);
    }, [openLogin]);
}
export function EpicAuthError({error}: {error: string}) {
    if (!error) return null;
    const authRequired = isEpicAuthRequired(error);
    return <p className="error-text" role="alert">{tr(authRequired ? 'Не удалось восстановить вход Epic. Проверьте сеть или войдите заново' : error)}{authRequired && <button onClick={() => window.dispatchEvent(new Event(EPIC_LOGIN_EVENT))}>{tr('Войти в Epic')}</button>}</p>;
}
