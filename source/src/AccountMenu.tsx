import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { ArrowRightLeft, ChevronDown, KeyRound, LogIn, LogOut, UserRound, Info } from 'lucide-react';
export function AccountMenu({ account, busy, onLogin, onLogout, onRedeem, onAbout }: {
    account: {
        signedIn: boolean;
        displayName: string | null;
    };
    busy: boolean;
    onLogin: () => void;
    onLogout: () => Promise<void>;
    onRedeem: () => void;
    onAbout: () => void;
}) {
    const [open, setOpen] = useState(false);
    const root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null), menu = useRef<HTMLDivElement>(null);
    const close = () => { setOpen(false); trigger.current?.focus(); };
    useEffect(() => { setOpen(false); }, [account.signedIn]);
    useEffect(() => { if (!open)
        return; menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus(); const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node))
        setOpen(false); }; const blur = () => setOpen(false); document.addEventListener('pointerdown', outside); window.addEventListener('blur', blur); return () => { document.removeEventListener('pointerdown', outside); window.removeEventListener('blur', blur); }; }, [open]);
    return <div className="user-area" ref={root} onBlur={e => { if (e.relatedTarget && !e.currentTarget.contains(e.relatedTarget as Node))
        setOpen(false); }}><button ref={trigger} className="user-trigger" disabled={busy} aria-label={tr("Меню аккаунта")} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(v => !v)}><span className="user-avatar">{tr(account.signedIn ? (account.displayName || 'E').slice(0, 1).toUpperCase() : <UserRound size={18}/>)}</span><span className="user-summary"><b>{tr(account.signedIn ? account.displayName || 'Epic Games' : 'Войти в Epic')}</b><small>{tr(account.signedIn ? 'Epic подключён' : 'Подключить библиотеку')}</small></span>{tr(account.signedIn ? <ChevronDown size={15}/> : <LogIn size={17}/>)}</button>{tr(open && <div ref={menu} className="user-menu" role="menu" aria-label={tr("Аккаунт Epic Games")} onKeyDown={e => { if (e.key === 'Escape') {
        e.preventDefault();
        close();
    } if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
        e.preventDefault();
        const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length]?.focus();
    } }}><div className="user-menu-heading"><b>{tr(account.displayName || 'Epic Games')}</b><small>{tr(account.signedIn ? 'Библиотека подключена' : 'Подключить библиотеку')}</small></div>{tr(account.signedIn && <button role="menuitem" onClick={() => { close(); onRedeem(); }}><KeyRound size={17}/>{tr("Активировать ключ")}</button>)}<button role="menuitem" disabled={busy} onClick={() => { close(); onLogin(); }}><ArrowRightLeft size={17}/>{tr(account.signedIn ? 'Сменить аккаунт' : 'Войти в Epic')}</button><button role="menuitem" onClick={() => { close(); onAbout(); }}><Info size={17}/>{tr("О приложении")}</button>{tr(account.signedIn && <button role="menuitem" className="user-signout" disabled={busy} onClick={() => { close(); void onLogout(); }}><LogOut size={17}/>{tr("Выйти из Epic")}</button>)}</div>)}</div>;
}
