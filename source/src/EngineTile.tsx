import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Download } from 'lucide-react';
import unrealMark from './assets/unreal-mark.svg';
export function EngineTile({ version, installed, busy, active, onInstall }: {
    version: string;
    installed: boolean;
    busy: boolean;
    active: boolean;
    onInstall: () => void;
}) {
    const [menu, setMenu] = useState<{
        x: number;
        y: number;
    } | null>(null);
    const tile = useRef<HTMLButtonElement>(null), menuRef = useRef<HTMLDivElement>(null);
    const close = () => { setMenu(null); tile.current?.focus(); };
    useEffect(() => { if (!active)
        setMenu(null); }, [active]);
    useEffect(() => {
        if (!menu)
            return;
        menuRef.current?.querySelector<HTMLButtonElement>('button')?.focus();
        const outside = (e: Event) => { if (!menuRef.current?.contains(e.target as Node))
            setMenu(null); };
        const hide = () => setMenu(null);
        document.addEventListener('pointerdown', outside);
        window.addEventListener('resize', hide);
        window.addEventListener('scroll', hide, true);
        window.addEventListener('blur', hide);
        return () => { document.removeEventListener('pointerdown', outside); window.removeEventListener('resize', hide); window.removeEventListener('scroll', hide, true); window.removeEventListener('blur', hide); };
    }, [menu]);
    const show = (x: number, y: number) => setMenu({ x: Math.max(8, Math.min(x, window.innerWidth - 280)), y: Math.max(8, Math.min(y, window.innerHeight - 64)) });
    return <>
  <button ref={tile} className="fab-tile engine-available-tile" disabled={busy} aria-label={tr(`Установить Unreal Engine ${version}`)} aria-haspopup="menu" onDoubleClick={onInstall} onClick={e => { if (e.detail === 0)
        onInstall(); }} onContextMenu={e => { e.preventDefault(); show(e.clientX, e.clientY); }} onKeyDown={e => { if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        show(r.left + 12, r.top + 12);
    } }}>
   <img src={unrealMark} alt={tr("")} aria-hidden="true"/>
   <span className="engine-tile-version">{tr(version)}</span><span className="engine-tile-caption">{tr("UNREAL ENGINE")}</span>
   {tr(installed && <span className="downloaded-badge">{tr("Установлен")}</span>)}
  </button>
  {tr(menu && active && createPortal(<div ref={menuRef} role="menu" aria-label={tr(`Unreal Engine ${version}`)} className="project-context-menu" style={{ left: menu.x, top: menu.y }} onKeyDown={e => { if (e.key === 'Escape') {
        e.preventDefault();
        close();
    } if (e.key === 'Tab')
        setMenu(null); }}><button role="menuitem" disabled={busy} onClick={() => { close(); onInstall(); }}><Download size={17}/>{tr("Установить")}</button></div>, document.body))}
 </>;
}
