import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Maximize2, Minus, Moon, PanelsTopLeft, Sun, X, type LucideIcon } from 'lucide-react';
import { tr } from './i18n';
import { useTheme } from './theme';
import metadata from '../package.json';
import logo from './assets/um-logo.svg';

type Props = {
  section: string;
  items: ReadonlyArray<{ id: string; label: string; icon: LucideIcon; badge?: number }>;
  onNavigate: (section: string) => void;
  accountMenu: ReactNode;
};

export function WorkspaceHeader({ section, items, onNavigate, accountMenu }: Props) {
  const { theme, toggleTheme } = useTheme();
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState(false);
  const operation = useRef(false);
  const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  useEffect(() => {
    if (!native) return;
    let disposed = false, revision = 0;
    const current = getCurrentWindow();
    const update = async () => {
      const id = ++revision;
      try { const value = await current.isMaximized(); if (!disposed && id === revision) setMaximized(value); }
      catch { if (!disposed) setError(true); }
    };
    void update();
    const listener = current.onResized(() => { void update(); });
    void listener.catch(() => { if (!disposed) setError(true); });
    return () => { disposed = true; void listener.then(unlisten => unlisten()).catch(() => {}); };
  }, [native]);

  useEffect(() => {
    if (!native) return;
    const current = getCurrentWindow();
    const background = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim();
    void current.setTheme(theme).catch(() => setError(true));
    void current.setBackgroundColor(background).catch(() => setError(true));
  }, [theme, native]);

  async function act(action: 'minimize' | 'maximize' | 'close' | 'drag') {
    if (!native || operation.current) return;
    operation.current = true;
    setError(false);
    try {
      const current = getCurrentWindow();
      if (action === 'minimize') await current.minimize();
      else if (action === 'close') await current.close(); // Native CloseRequested keeps the existing Ask / Tray / Exit policy.
      else if (action === 'drag') await current.startDragging();
      else { await current.toggleMaximize(); setMaximized(await current.isMaximized()); }
    } catch { setError(true); }
    finally { operation.current = false; }
  }

  function drag(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0 || ![1, 2].includes(event.detail)) return;
    const target = event.target;
    if (!(target instanceof Element) || target.closest('button,a,input,select,textarea,label,summary,[role="menu"],[contenteditable],[data-no-window-drag]')) return;
    event.preventDefault();
    void act(event.detail === 2 ? 'maximize' : 'drag');
  }

  const themeLabel = theme === 'dark' ? tr('Светлая тема') : tr('Тёмная тема');
  const maximizeLabel = maximized ? tr('Восстановить окно') : tr('Развернуть окно');
  return <header className="workspace-topbar unified-topbar" onMouseDown={drag}>
    <button type="button" className="workspace-brand unified-brand" onClick={() => onNavigate('home')} aria-label={tr('Unreal Manager — Главная')}>
      <img className="unified-logo" src={logo} alt="" draggable={false} />
      <span className="unified-brand-copy"><b>Unreal Manager <span className="unified-version">{metadata.version}</span></b><small>{tr('Движки, проекты и материалы')}</small></span>
    </button>
    <nav className="workspace-nav unified-nav" aria-label={tr('Основные разделы')}>
      {items.map(({ id, label, icon: Icon, badge }) => <button type="button" key={id} onClick={() => onNavigate(id)} className={section === id ? 'active' : ''} aria-current={section === id ? 'page' : undefined} aria-label={tr(label)} title={tr(label)}>
        <Icon size={19} aria-hidden="true" /><span>{tr(label)}</span>{!!badge && badge > 0 && <em className="nav-update-count">{badge}</em>}
      </button>)}
    </nav>
    <div className="unified-tools">
      <button type="button" className="theme-toggle" onClick={toggleTheme} title={themeLabel} aria-label={themeLabel}>{theme === 'dark' ? <Sun size={21} aria-hidden="true" /> : <Moon size={21} aria-hidden="true" />}</button>
      {accountMenu}
      <div className="window-controls" role="group" aria-label={tr('Управление окном')}>
        <button type="button" onClick={() => void act('minimize')} title={tr('Свернуть окно')} aria-label={tr('Свернуть окно')}><Minus size={18} aria-hidden="true" /></button>
        <button type="button" onClick={() => void act('maximize')} title={maximizeLabel} aria-label={maximizeLabel}>{maximized ? <PanelsTopLeft size={17} aria-hidden="true" /> : <Maximize2 size={17} aria-hidden="true" />}</button>
        <button type="button" className="window-close" onClick={() => void act('close')} title={tr('Закрыть окно')} aria-label={tr('Закрыть окно')}><X size={21} aria-hidden="true" /></button>
      </div>
    </div>
    {error && <div className="window-control-error" role="alert" data-no-window-drag><span>{tr('Не удалось изменить состояние окна. Повторите действие.')}</span><button type="button" className="icon" onClick={() => setError(false)} aria-label={tr('Закрыть')}><X size={16} /></button></div>}
  </header>;
}
