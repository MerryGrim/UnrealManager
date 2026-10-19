import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { FolderKanban, FolderOpen, Play, Trash2 } from 'lucide-react';
import { launchProject, type UnrealProject } from './lib/backend';
export function ProjectTile({ project, onError, onRemoved }: {
    project: UnrealProject;
    onError: (message: string) => void;
    onRemoved: (path: string) => void;
}) {
    const lock = useRef(false), tile = useRef<HTMLButtonElement>(null), menuRef = useRef<HTMLDivElement>(null), cancelRef = useRef<HTMLButtonElement>(null);
    const [busy, setBusy] = useState(false), [menu, setMenu] = useState<{
        x: number;
        y: number;
    } | null>(null), [directory, setDirectory] = useState<string | null>(null), [deleteError, setDeleteError] = useState('');
    const close = () => { setMenu(null); tile.current?.focus(); };
    useEffect(() => {
        if (!menu)
            return;
        menuRef.current?.querySelector('button')?.focus();
        const dismiss = (event: Event) => { if (!menuRef.current?.contains(event.target as Node))
            setMenu(null); };
        const hide = () => setMenu(null);
        document.addEventListener('pointerdown', dismiss);
        window.addEventListener('resize', hide);
        window.addEventListener('scroll', hide, true);
        window.addEventListener('blur', hide);
        return () => { document.removeEventListener('pointerdown', dismiss); window.removeEventListener('resize', hide); window.removeEventListener('scroll', hide, true); window.removeEventListener('blur', hide); };
    }, [menu]);
    useEffect(() => { if (directory)
        cancelRef.current?.focus(); }, [directory]);
    const action = async (kind: 'open' | 'reveal' | 'delete') => {
        if (lock.current)
            return;
        close();
        lock.current = true;
        setBusy(true);
        onError('');
        try {
            if (kind === 'open') {
                await launchProject(project.path);
                await new Promise(resolve => setTimeout(resolve, 2000));
            }
            else if (kind === 'reveal')
                await invoke('reveal_project', { path: project.path });
            else {
                setDeleteError('');
                setDirectory(await invoke<string>('project_delete_preview', { path: project.path }));
            }
        }
        catch (e) {
            onError(String(e));
        }
        finally {
            lock.current = false;
            setBusy(false);
        }
    };
    const remove = async (mode: 'recycle' | 'permanent') => {
        if (lock.current || !directory)
            return;
        lock.current = true;
        setBusy(true);
        setDeleteError('');
        try {
            await invoke('delete_project', { path: project.path, directory, mode });
            setDirectory(null);
            onRemoved(project.path);
        }
        catch (e) {
            setDeleteError(String(e));
        }
        finally {
            lock.current = false;
            setBusy(false);
        }
    };
    const openMenu = (x: number, y: number) => { if (!busy)
        setMenu({ x: Math.max(8, Math.min(x, window.innerWidth - 280)), y: Math.max(8, Math.min(y, window.innerHeight - 148)) }); };
    return <>
 <button ref={tile} className="fab-tile project-tile" disabled={busy} aria-label={tr(`Открыть ${project.name}`)} aria-haspopup="menu" title={tr(project.path)} onContextMenu={e => { e.preventDefault(); openMenu(e.clientX, e.clientY); }} onKeyDown={e => { if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        const rect = e.currentTarget.getBoundingClientRect();
        openMenu(rect.left + 12, rect.top + 12);
    } }} onDoubleClick={() => void action('open')} onClick={e => { if (e.detail === 0)
        void action('open'); }}>
   <span className="fab-cover project-cover">{tr(project.thumbnail ? <img src={project.thumbnail} alt={tr("")} loading="lazy" draggable={false}/> : <FolderKanban size={52} strokeWidth={1.3}/>)}</span>
   <span className="project-version">{tr(project.engineAssociation ? `UE ${project.engineAssociation}` : 'Версия UE не указана')}</span>
   <span className="fab-tile-title">{project.name}</span><span className="project-tile-status" role="status">{tr(busy ? 'Выполняется…' : 'Открыть в Unreal Engine')}</span>
 </button>
 {tr(menu && createPortal(<div ref={menuRef} className="project-context-menu" role="menu" aria-label={tr(`Действия: ${project.name}`)} style={{ left: menu.x, top: menu.y }} onContextMenu={e => e.preventDefault()} onKeyDown={e => {
                if (e.key === 'Escape') {
                    e.preventDefault();
                    close();
                }
                if (e.key === 'Tab') {
                    setMenu(null);
                }
                if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
                    e.preventDefault();
                    const buttons = Array.from(e.currentTarget.querySelectorAll('button'));
                    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
                    buttons[e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (index + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length]?.focus();
                }
            }}><button role="menuitem" onClick={() => void action('open')}><Play size={17}/>{tr("Открыть")}</button><button role="menuitem" onClick={() => void action('reveal')}><FolderOpen size={17}/>{tr("Перейти к расположению")}</button><button role="menuitem" className="danger-text" onClick={() => void action('delete')}><Trash2 size={17}/>{tr("Удалить проект")}</button></div>, document.body))}
 {tr(directory && createPortal(<div className="modal-backdrop" onKeyDown={e => {
                if (e.key === 'Escape' && !busy) {
                    setDirectory(null);
                    tile.current?.focus();
                }
                if (e.key === 'Tab') {
                    const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
                    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
                    e.preventDefault();
                    buttons[(i + (e.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
                }
            }}><section className="login-dialog" role="dialog" aria-modal="true" aria-label={tr(`Удалить проект ${project.name}`)}><h2>{tr("Удалить проект «")}{project.name}{tr("»?")}</h2><p>{tr("Выберите способ удаления всей папки проекта со всеми файлами:")}</p><p className="project-delete-path">{tr(directory)}</p><p>{tr("Из корзины проект можно восстановить. При выборе «Удалить навсегда» папка удаляется без помещения в корзину.")}</p><p>{tr("Закройте проект в Unreal Engine перед удалением.")}</p>{tr(deleteError && <p role="alert" className="error-text">{tr(deleteError)}</p>)}<div className="dialog-actions project-delete-actions"><button ref={cancelRef} disabled={busy} onClick={() => { setDirectory(null); tile.current?.focus(); }}>{tr("Отмена")}</button><button disabled={busy} onClick={() => void remove('recycle')}><Trash2 size={17}/>{tr("В корзину")}</button><button className="danger-button" disabled={busy} onClick={() => void remove('permanent')}><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M7 9V2h10v7M4 9h16v7H4zM7 19v3m5-3v3m5-3v3M8 12h8"/></svg>{tr("Удалить навсегда")}</button></div></section></div>, document.body))}
 </>;
}
