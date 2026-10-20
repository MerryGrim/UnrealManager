import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { FolderOpen, FolderPlus, PackagePlus, PlugZap, AlertCircle, Trash2, Download, Info,Play } from 'lucide-react';
import { FabDetails } from './FabDetails';
import { FabCover, type FabItem } from './EpicLibrary';
export const materialAction = (kind: string) => kind === 'Project' ? 'Создать проект' : kind === 'Plugin' ? 'Установить плагин' : 'Добавить в проект';
export function LibraryTile({ item, onOpen, busy, progress, downloaded, packageId, packageIds = [], onVaultChanged,resumeId,onResume }: {
    resumeId?:string;
    onResume?:(id:string)=>void;
    item: FabItem;
    onOpen: (item: FabItem, install?: boolean, downloadOnly?: boolean) => void;
    busy: boolean;
    progress: {
        id: string;
        percent: number;
        phase: string;
    } | null;
    downloaded: boolean;
    packageId?: string;
    packageIds?: string[];
    onVaultChanged: () => Promise<void>;
}) {
    const [information, setInformation] = useState(false);
    const tile = useRef<HTMLButtonElement>(null), menuRef = useRef<HTMLDivElement>(null), errorButton = useRef<HTMLButtonElement>(null);
    const [menu, setMenu] = useState<{
        x: number;
        y: number;
    } | null>(null), [error, setError] = useState('');
    const [deletion, setDeletion] = useState<{
        ids: string[];
        paths: string[];
    } | null>(null), [deleting, setDeleting] = useState(false), [deleteError, setDeleteError] = useState('');
    const deleteLock = useRef(false), cancelDelete = useRef<HTMLButtonElement>(null);
    useEffect(() => { if (deletion)
        cancelDelete.current?.focus(); }, [deletion]);
    const close = () => { setMenu(null); tile.current?.focus(); };
    useEffect(() => {
        if (!menu)
            return;
        menuRef.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
        const outside = (e: Event) => { if (!menuRef.current?.contains(e.target as Node))
            setMenu(null); };
        const hide = () => setMenu(null);
        document.addEventListener('pointerdown', outside);
        window.addEventListener('resize', hide);
        window.addEventListener('scroll', hide, true);
        window.addEventListener('blur', hide);
        return () => { document.removeEventListener('pointerdown', outside); window.removeEventListener('resize', hide); window.removeEventListener('scroll', hide, true); window.removeEventListener('blur', hide); };
    }, [menu]);
    useEffect(() => { if (error)
        errorButton.current?.focus(); }, [error]);
    const show = (x: number, y: number) => setMenu({ x: Math.max(8, Math.min(x, window.innerWidth - 280)), y: Math.max(8, Math.min(y, window.innerHeight - 260)) });
    const reveal = async () => {
        close();
        if (!packageId) {
            setError('Материал ещё не скачан. Сначала скачайте его в хранилище.');
            return;
        }
        try {
            await invoke('vault_reveal', { id: packageId });
        }
        catch (e) {
            setError(String(e));
        }
    };
    const askDelete = async () => {
        close();
        setDeleteError('');
        const ids = packageIds.length ? packageIds : packageId ? [packageId] : [];
        try {
            const paths = await invoke<string[]>('vault_delete_preview', { ids });
            setDeletion({ ids, paths });
        }
        catch (e) {
            setError(String(e));
        }
    };
    const remove = async () => {
        if (!deletion || deleteLock.current)
            return;
        deleteLock.current = true;
        setDeleting(true);
        setDeleteError('');
        try {
            await invoke('vault_delete', { ids: deletion.ids, expectedPaths: deletion.paths });
            setDeletion(null);
            tile.current?.focus();
        }
        catch (e) {
            setDeleteError(String(e));
        }
        finally {
            try {
                await onVaultChanged();
            }
            finally {
                setDeleting(false);
                deleteLock.current = false;
            }
        }
    };
    const Icon = item.kind === 'Project' ? FolderPlus : item.kind === 'Plugin' ? PlugZap : PackagePlus;
    const current = progress?.id === item.id ? progress : null;
    return <>
   <button ref={tile} className="fab-tile" onDoubleClick={() => onOpen(item)} onClick={e => { if (e.detail === 0)
        onOpen(item); }} aria-label={tr(`Открыть ${item.title}`)} aria-haspopup="menu" onContextMenu={e => { e.preventDefault(); show(e.clientX, e.clientY); }} onKeyDown={e => { if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        show(r.left + 12, r.top + 12);
    } }}>
     <FabCover item={item}/>{tr(current && <><span className="tile-download-fill" style={{ height: `${current.percent}%` }} aria-hidden="true"/><span className="tile-download-label" role="progressbar" aria-label={tr(current.phase)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={current.percent}>{tr(current.percent)}{tr("%")}{['На паузе','Ожидает продолжения'].includes(current.phase)&&<small>{tr(current.phase)}</small>}</span></>)}
     <span className="fab-tile-title">{item.title}</span>{tr(downloaded && <span className="downloaded-badge">{tr("Скачано")}</span>)}
   </button>
   {tr(menu && createPortal(<div ref={menuRef} role="menu" aria-label={tr(`Действия: ${item.title}`)} className="project-context-menu" style={{ left: menu.x, top: menu.y }} onContextMenu={e => e.preventDefault()} onKeyDown={e => {
                if (e.key === 'Escape') {
                    e.preventDefault();
                    close();
                }
                if (e.key === 'Tab')
                    setMenu(null);
                if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
                    e.preventDefault();
                    const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
                    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
                    buttons[e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length]?.focus();
                }
            }}><button role="menuitem" onClick={() => { close(); setInformation(true); }}><Info size={17}/>{tr("Информация")}</button>{resumeId&&onResume&&<button role="menuitem" disabled={busy} onClick={()=>{close();onResume(resumeId);}}><Play size={17}/>{tr('Продолжить загрузку')}</button>}{tr(item.kind !== 'Plugin' && <button role="menuitem" disabled={busy} onClick={() => { close(); onOpen(item, true); }}><Icon size={17}/>{tr(materialAction(item.kind))}</button>)}<button role="menuitem" onClick={() => { close(); onOpen(item, false, true); }}><Download size={17}/>{tr("Скачать в хранилище")}</button><button role="menuitem" onClick={() => void reveal()}><FolderOpen size={17}/>{tr("Открыть расположение")}</button>{tr(downloaded && <button role="menuitem" className="danger-text" disabled={busy || deleting} onClick={() => void askDelete()}><Trash2 size={17}/>{tr("Удалить из хранилища")}</button>)}</div>, document.body))}
   {tr(information && <FabDetails item={item} busy={busy} onClose={() => { setInformation(false); tile.current?.focus(); }} onDownload={onOpen}/>)}
   {tr(deletion && createPortal(<div className="modal-backdrop" onKeyDown={e => { if (e.key === 'Escape' && !deleting) {
        setDeletion(null);
        tile.current?.focus();
    } if (e.key === 'Tab') {
        const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        if (!buttons.length) {
            e.preventDefault();
            return;
        }
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        e.preventDefault();
        buttons[(index + (e.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
    } }}><section className="login-dialog" role="alertdialog" aria-modal="true" aria-label={tr(`Удалить из хранилища ${item.title}`)}><h2>{tr("Удалить «")}{item.title}{tr("» из хранилища?")}</h2><p>{tr("Все скачанные копии этого материала будут удалены навсегда, без корзины:")}</p>{tr(deletion.paths.map(path => <p className="project-delete-path" key={path}>{tr(path)}</p>))}<p>{tr("Созданные проекты и установленные копии не затрагиваются. Материал останется в библиотеке, его можно скачать снова.")}</p>{tr(deleteError && <p className="error-text" role="alert">{tr(deleteError)}</p>)}<div className="wizard-footer"><button ref={cancelDelete} disabled={deleting} onClick={() => { setDeletion(null); tile.current?.focus(); }}>{tr("Отмена")}</button><button className="danger-button" disabled={deleting} onClick={() => void remove()}><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="M7 9V2h10v7M4 9h16v7H4zM7 19v3m5-3v3m5-3v3M8 12h8"/></svg>{tr(deleting ? 'Удаляем…' : 'Удалить навсегда')}</button></div></section></div>, document.body))}
   {tr(error && createPortal(<div className="modal-backdrop" onKeyDown={e => { if (e.key === 'Escape') {
        setError('');
        tile.current?.focus();
    } if (e.key === 'Tab') {
        e.preventDefault();
        errorButton.current?.focus();
    } }}><section className="login-dialog" role="alertdialog" aria-modal="true" aria-label={tr("Не удалось выполнить действие")}><div className="section-head"><h2><AlertCircle size={20}/>{tr(" Не удалось выполнить действие")}</h2></div><p>{item.title}</p><p className="error-text">{tr(error)}</p><div className="wizard-footer"><button ref={errorButton} onClick={() => { setError(''); tile.current?.focus(); }}>{tr("Закрыть")}</button></div></section></div>, document.body))}
 </>;
}
