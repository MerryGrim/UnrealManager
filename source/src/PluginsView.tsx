import {getPreference,setPreference} from './preferences';
import { tr, getLocale } from './i18n';
import { PageActions } from './PageActions';
import { FabCover, type FabItem } from './EpicLibrary';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { PlugZap, RefreshCw, Trash2, FolderOpen, Download, PackagePlus,Play } from 'lucide-react';
import type { VaultPackage, UpdateTransfer } from './Vault';
type Installed = {
    name: string;
    title: string;
    engine: string;
    enginePath: string;
    path: string;
    vaultIds: string[];
};
type Group = {
    key: string;
    title: string;
    packages: VaultPackage[];
    installed: Installed[];
};
type Removal = {
    group: Group;
    paths: string[];
};
const Shredder = () => <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="M7 9V2h10v7M4 9h16v7H4zM7 19v3m5-3v3m5-3v3M8 12h8"/></svg>;
export function PluginsView({ items, busy, onInstall, onLibrary, onRefresh, onUpdate, onDownload, transfers, downloadProgress,pendingProgress={},onResume }: {
    pendingProgress?:Record<string,{id:string;percent:number;phase:string;taskId:string}>;
    onResume?:(id:string)=>void;
    onDownload: (item: FabItem) => void;
    transfers: Record<string, UpdateTransfer>;
    downloadProgress: {
        id: string;
        percent: number;
        phase: string;
    } | null;
    items: VaultPackage[];
    busy: boolean;
    onInstall: (item: VaultPackage, destination?: 'project' | 'engine') => void;
    onLibrary: () => void;
    onRefresh: () => Promise<void>;
    onUpdate: (item: VaultPackage) => void;
}) {
    const [query, setQuery] = useState(''), [version, setVersion] = useState('All'), [page, setPage] = useState(0);
    const [pageSize, setPageSize] = useState(() => { try {
        const n = Number(getPreference('um-plugins-page-size'));
        return [24, 48, 96, 192].includes(n) ? n : 48;
    }
    catch {
        return 48;
    } });
    const [scope, setScope] = useState<'vault' | 'engine' | 'all'>('vault'), [vaultMode, setVaultMode] = useState<'recycle' | 'permanent'>('recycle');
    const [covers, setCovers] = useState<FabItem[]>([]);
    useEffect(() => { let active = true; invoke<{
        items: FabItem[];
    } | null>('fab_cached_library').then(cache => { if (active)
        setCovers(cache?.items || []); }).catch(() => { }); return () => { active = false; }; }, [items]);
    const [installed, setInstalled] = useState<Installed[]>([]), [error, setError] = useState(''), [message, setMessage] = useState(''), [working, setWorking] = useState(false);
    const [menu, setMenu] = useState<{
        group: Group;
        x: number;
        y: number;
    } | null>(null), [removal, setRemoval] = useState<Removal | null>(null), [enginePath, setEnginePath] = useState('');
    const menuRef = useRef<HTMLDivElement>(null), cancelRef = useRef<HTMLButtonElement>(null), trigger = useRef<HTMLButtonElement | null>(null), lock = useRef(false);
    const refreshInstalled = async () => { setInstalled(await invoke<Installed[]>('plugins_installed')); };
    useEffect(() => { let active = true; if (!busy)
        invoke<Installed[]>('plugins_installed').then(v => { if (active)
            setInstalled(v); }).catch(e => { if (active)
            setError(String(e)); }); return () => { active = false; }; }, [items, busy]);
    useEffect(() => { if (!menu)
        return; menuRef.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus(); const hide = () => setMenu(null); const outside = (e: Event) => { if (!menuRef.current?.contains(e.target as Node))
        hide(); }; window.addEventListener('pointerdown', outside); window.addEventListener('resize', hide); window.addEventListener('scroll', hide, true); window.addEventListener('blur', hide); return () => { window.removeEventListener('pointerdown', outside); window.removeEventListener('resize', hide); window.removeEventListener('scroll', hide, true); window.removeEventListener('blur', hide); }; }, [menu]);
    useEffect(() => { if (removal)
        cancelRef.current?.focus(); }, [removal]);
    const groups = new Map<string, Group>();
    for (const item of items.filter(p => p.kind === 'Plugin')) {
        const key = item.assetId || item.title;
        const group = groups.get(key) || { key, title: item.title, packages: [], installed: [] };
        group.packages.push(item);
        groups.set(key, group);
    }
    for (const entry of installed) {
        let group = [...groups.values()].find(g => g.packages.some(p => entry.vaultIds.includes(p.id))) || groups.get(`engine:${entry.name}`);
        if (!group) {
            group = { key: `engine:${entry.name}`, title: entry.title, packages: [], installed: [] };
            groups.set(group.key, group);
        }
        group.installed.push(entry);
    }
    const close = () => { setMenu(null); trigger.current?.focus(); };
    const show = (group: Group, x: number, y: number) => setMenu({ group, x: Math.max(8, Math.min(x, window.innerWidth - 310)), y: Math.max(8, Math.min(y, window.innerHeight - 280)) });
    const ask = async (group: Group) => {
        close();
        setError('');
        setMessage('');
        setWorking(true);
        try {
            const [current, paths] = await Promise.all([invoke<Installed[]>('plugins_installed'), group.packages.length ? invoke<string[]>('vault_delete_preview', { ids: group.packages.map(p => p.id) }) : Promise.resolve([])]);
            const found = current.filter(p => group.installed.some(i => i.name === p.name) || p.vaultIds.some(id => group.packages.some(v => v.id === id)));
            setScope(paths.length ? 'vault' : 'engine');
            setVaultMode('recycle');
            setEnginePath(found.length === 1 ? found[0].path : '');
            setRemoval({ group: { ...group, installed: found }, paths });
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setWorking(false);
        }
    };
    const remove = async () => {
        if (!removal || lock.current)
            return;
        lock.current = true;
        setWorking(true);
        setError('');
        const completed: string[] = [];
        try {
            const targets = scope === 'all' ? removal.group.installed : scope === 'engine' ? removal.group.installed.filter(p => p.path === enginePath) : [];
            if (scope === 'engine' && !targets.length)
                throw Error('Выберите установленный движок.');
            for (const entry of targets) {
                await invoke('plugin_remove_engine', { name: entry.name, enginePath: entry.enginePath, expectedPath: entry.path });
                completed.push(entry.path);
            }
            if (scope !== 'engine' && removal.paths.length) {
                await invoke('vault_delete', { ids: removal.group.packages.map(p => p.id), expectedPaths: removal.paths, mode: vaultMode });
                completed.push(...removal.paths);
            }
            setMessage(scope === 'all' ? 'Плагин удалён из Vault и со всех выбранных движков.' : scope === 'engine' ? 'Плагин удалён с выбранного движка.' : vaultMode === 'recycle' ? 'Копии из Vault перемещены в корзину.' : 'Копии из Vault удалены навсегда.');
            setRemoval(null);
            trigger.current?.focus();
        }
        catch (e) {
            setError((completed.length ? 'Часть операции выполнена. Обработаны папки: ' + completed.join('; ') + '. ' : '') + String(e));
            setRemoval(null);
        }
        finally {
            try {
                await onRefresh();
                await refreshInstalled();
            }
            catch (e) {
                setError(String(e));
            }
            setWorking(false);
            lock.current = false;
        }
    };
    const reveal = async (group: Group) => { close(); setError(''); try {
        if (!group.packages.length)
            throw Error('Плагин не скачан в Vault.');
        await invoke('vault_reveal', { id: group.packages[0].id });
    }
    catch (e) {
        setError(String(e));
    } };
    const coverFor = (group: Group) => covers.find(c => group.packages.some(p => p.assetId === c.id) || c.title === group.title);
    const versions = [...new Set([...groups.values()].flatMap(g => [...g.packages.flatMap(p => p.engines.map(v => v.replace(/^UE_/, ''))), ...g.installed.map(p => p.engine)]))].sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
    const filtered = [...groups.values()].filter(g => g.title.toLowerCase().includes(query.toLowerCase()) && (version === 'All' || g.packages.some(p => p.engines.some(v => v.replace(/^UE_/, '') === version)) || g.installed.some(p => p.engine === version)));
    const pages = Math.max(1, Math.ceil(filtered.length / pageSize)), currentPage = Math.min(page, pages - 1);
    const tileProgress = (group: Group) => { const active = group.packages.map(p => transfers[p.id]).find(t => t?.state === 'preparing' || t?.state === 'downloading'); return active ? { percent: active.percent, phase: active.phase } : downloadProgress && group.packages.some(p => p.assetId === downloadProgress.id) ? downloadProgress : pendingProgress[group.key]||null; };
    const disabled = busy || working;
    return <div className="content library-content plugins-content"><PageActions><button disabled={disabled} onClick={() => void Promise.all([onRefresh(), refreshInstalled()]).catch(e => setError(String(e)))}><RefreshCw size={17}/>{tr("Обновить")}</button><button onClick={onLibrary}>{tr("Скачать из библиотеки")}</button></PageActions><div className="library-summary"><span className="muted">{tr("Плагинов: ")}{tr(groups.size)}</span></div><div className="library-filters"><input aria-label={tr("Поиск плагинов")} placeholder={tr("Название плагина")} value={query} onChange={e => { setQuery(e.target.value); setPage(0); }}/><select aria-label={tr("Версия UE для плагинов")} value={version} onChange={e => { setVersion(e.target.value); setPage(0); }}><option value="All">{tr("Все версии UE")}</option>{tr(versions.map(v => <option key={v} value={v}>{tr("UE ")}{tr(v)}</option>))}</select><label className="page-size">{tr("На странице")}<select aria-label={tr("Плагинов на странице")} value={pageSize} onChange={e => { const n = Number(e.target.value); setPageSize(n); setPage(0); try {
        setPreference('um-plugins-page-size', String(n));
    }
    catch { } }}>{tr([24, 48, 96, 192].map(n => <option key={n} value={n}>{tr(n)}</option>))}</select></label></div>{tr(error && !removal && <p role="alert" className="error-text">{tr(error)}</p>)}{tr(message && <p role="status" className="vault-success">{tr(message)}</p>)}<div className="library-items">{tr(!groups.size ? <div className="library-empty"><PlugZap size={36}/><h3>{tr("Плагины пока не найдены")}</h3><p>{tr("Скачайте нужную версию плагина из библиотеки.")}</p><button onClick={onLibrary}>{tr("Открыть библиотеку")}</button></div> : <div className="fab-grid">{tr(filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize).map(group => { const current = tileProgress(group); return <button className="fab-tile plugin-tile" key={group.key} disabled={working || (busy && !current)} aria-label={tr(group.title)} aria-haspopup="menu" onDoubleClick={() => !busy && (group.packages.length ? onInstall(group.packages[0]) : setError('Для установки в другое место сначала скачайте нужную версию из библиотеки.'))} onClick={e => { if (e.detail === 0 && !busy) {
        if (group.packages.length)
            onInstall(group.packages[0]);
        else
            setError('Сначала скачайте нужную версию из библиотеки.');
    } }} onContextMenu={e => { e.preventDefault(); trigger.current = e.currentTarget; show(group, e.clientX, e.clientY); }} onKeyDown={e => { if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        trigger.current = e.currentTarget;
        const r = e.currentTarget.getBoundingClientRect();
        show(group, r.left + 12, r.top + 12);
    } }}>{tr(coverFor(group) ? <FabCover item={coverFor(group)!}/> : <span className="plugin-tile-art"><PlugZap size={58} strokeWidth={1.3}/></span>)}<>{tr(current && <><span className="tile-download-fill" style={{ height: `${current.percent ?? 0}%` }} aria-hidden="true"/><span className="tile-download-label" role="progressbar" aria-label={tr(current.phase)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={current.percent ?? undefined}>{tr(current.percent === null ? '…' : `${current.percent}%`)}</span></>)}</><span className="fab-tile-title">{group.title}</span><span className="downloaded-badge">{tr(group.packages.length ? `В Vault: ${group.packages.length}` : 'На движке')}</span><span className="plugin-tile-meta" title={tr(current?.phase)}>{tr(current ? current.phase : <>{tr([...new Set(group.packages.flatMap(p => p.engines.map(v => v.replace(/^UE_/, ''))))].join(', '))}{tr(group.installed.length ? ` · На движке: ${group.installed.length}` : '')}</>)}</span></button>; }))}</div>)}{tr(!!groups.size && !filtered.length && <div className="empty">{tr("Плагины не найдены.")}</div>)}</div>{tr(filtered.length > 0 && <div className="pagination"><span>{tr("Найдено: ")}{tr(filtered.length)}{tr(" · Страница ")}{tr(currentPage + 1)}{tr(" из ")}{tr(pages)}</span><button disabled={!currentPage} onClick={() => setPage(currentPage - 1)}>{tr("Назад")}</button><button disabled={currentPage >= pages - 1} onClick={() => setPage(currentPage + 1)}>{tr("Далее")}</button></div>)}
 {tr(menu && createPortal(<div ref={menuRef} className="project-context-menu" role="menu" aria-label={tr(`Действия: ${menu.group.title}`)} style={{ left: menu.x, top: menu.y, minWidth: 290 }} onKeyDown={e => { if (e.key === 'Escape') {
        e.preventDefault();
        close();
    } if (e.key === 'Tab')
        setMenu(null); if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
        e.preventDefault();
        const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length]?.focus();
    } }}>{pendingProgress[menu.group.key]&&onResume&&<button role="menuitem" disabled={disabled} onClick={()=>{const id=pendingProgress[menu.group.key].taskId;close();onResume(id);}}><Play size={17}/>{tr('Продолжить загрузку')}</button>}<button role="menuitem" disabled={disabled || !menu.group.packages.length} onClick={() => { const item = menu.group.packages[0]; close(); onInstall(item); }}><PackagePlus size={17}/>{tr("Установить")}</button><button role="menuitem" disabled={disabled || menu.group.packages.length < 2} onClick={() => { const item = menu.group.packages[0]; close(); onUpdate(item); }}><RefreshCw size={17}/>{tr("Обновить установленный…")}</button><button role="menuitem" onClick={() => { const item = covers.find(c => menu.group.packages.some(p => p.assetId === c.id)); close(); if (item)
        onDownload(item);
    else
        onLibrary(); }}><Download size={17}/>{tr("Скачать в хранилище")}</button><button role="menuitem" onClick={() => void reveal(menu.group)}><FolderOpen size={17}/>{tr("Открыть расположение в Vault")}</button><button role="menuitem" className="danger-text" disabled={disabled} onClick={() => void ask(menu.group)}><Trash2 size={17}/>{tr("Удалить…")}</button></div>, document.body))}
 {tr(removal && createPortal(<div className="modal-backdrop" onKeyDown={e => { if (e.key === 'Escape' && !working)
            setRemoval(null); }}><section className="login-dialog" role="alertdialog" aria-modal="true" aria-label={tr(`Удаление ${removal.group.title}`)}><h2>{tr("Удалить ")}{removal.group.title}{tr("?")}</h2><div className="plugin-removal-options"><label><input type="radio" name="plugin-removal" checked={scope === 'vault'} disabled={working || !removal.paths.length} onChange={() => setScope('vault')}/>{tr("Только из Vault")}</label><label><input type="radio" name="plugin-removal" checked={scope === 'engine'} disabled={working || !removal.group.installed.length} onChange={() => setScope('engine')}/>{tr("С выбранного движка")}</label><label><input type="radio" name="plugin-removal" checked={scope === 'all'} disabled={working || (!removal.paths.length && !removal.group.installed.length)} onChange={() => setScope('all')}/>{tr("Везде — Vault и все найденные движки")}</label></div>
 {tr(scope === 'engine' && <label className="wizard-field">{tr("Движок")}<select value={enginePath} disabled={working} onChange={e => setEnginePath(e.target.value)}><option value="">{tr("Выберите движок")}</option>{tr(removal.group.installed.map(p => <option key={p.path} value={p.path}>{tr("UE ")}{tr(p.engine)}{tr(" · ")}{tr(p.enginePath)}</option>))}</select></label>)}
 {tr(scope !== 'engine' && removal.paths.length > 0 && <label className="downloaded-filter"><input type="checkbox" checked={vaultMode === 'permanent'} disabled={working} onChange={e => setVaultMode(e.target.checked ? 'permanent' : 'recycle')}/>{tr("Удалить копии из Vault безвозвратно")}</label>)}
 <p>{tr(scope !== 'engine' && removal.paths.length > 0 ? (vaultMode === 'recycle' ? 'Копии из Vault попадут в корзину. ' : 'Копии из Vault будут стёрты навсегда. ') : '')}{tr(scope !== 'vault' ? 'На движках плагин удаляется навсегда. ' : '')}{tr("Копии в проектах сохранятся.")}</p>
 <details open><summary>{tr("Папки для удаления")}</summary>{tr((scope === 'engine' ? [enginePath].filter(Boolean) : scope === 'vault' ? removal.paths : [...removal.paths, ...removal.group.installed.map(p => p.path)]).map(path => <p key={path} className="project-delete-path">{tr(path)}</p>))}</details>
 <div className="wizard-footer"><button ref={cancelRef} disabled={working} onClick={() => setRemoval(null)}>{tr("Отмена")}</button><button className="danger-button" disabled={working || busy || (scope === 'engine' && !enginePath)} onClick={() => void remove()}>{tr(working ? 'Удаляем…' : 'Удалить')}</button></div></section></div>, document.body))}
 </div>;
}
