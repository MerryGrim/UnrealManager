import {getPreference,setPreference} from './preferences';
import {EpicAuthError} from './EpicAuthError';
import { tr, getLocale } from './i18n';
import { PageActions } from './PageActions';
import { LibraryTile } from './LibraryTile';
import {categoryKey,libraryTopics} from './fabCategories';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { LogIn, RefreshCw, PackageOpen, X } from 'lucide-react';
type Account = {
    signedIn: boolean;
    displayName: string | null;
};
export type FabItem = {
    id: string;
    title: string;
    kind: string;
    engineVersions: string[];
    categories: string[];
    source: string;
    cover?: string | null;
    coverRevision?: number;
    description?: string;
    versions: {
        artifactId: string;
        engines: string[];
        platforms: string[];
    }[];
};
type Item = FabItem;
type Library = {
    items: Item[];
    scope: string;
};
const signedOut: Account = { signedIn: false, displayName: null };
type RedemptionResult = {
    status: 'ready' | 'success' | 'failed' | 'unknown';
    title: string;
    errorCode: string;
};
export function RedeemView({ account, onLibrary, onActivated }: {
    account: Account;
    onLibrary: () => void;
    onActivated: () => void;
}) {
    const [error, setError] = useState('');
    const [busy, setBusy] = useState(false);
    const [result, setResult] = useState<RedemptionResult | null>(null);
    const open = async () => {
        setError('');
        setResult(null);
        setBusy(true);
        try {
            const onResult = new Channel<RedemptionResult>(value => { setResult(value); if (value.status === 'success')
                onActivated(); });
            await invoke('epic_open_redeem', { onResult });
            setResult(value => value?.status === 'success' || value?.status === 'failed' ? value : { status: 'unknown', title: value?.title || '', errorCode: '' });
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
        }
    };
    const reason = result?.errorCode.endsWith('.code_used') ? 'Этот ключ уже использован.' : result?.errorCode.includes('already') ? 'Этот ключ уже использован или продукт уже есть в аккаунте.' : result?.errorCode.includes('invalid') ? 'Epic отклонил ключ. Проверьте введённые символы.' : 'Epic не подтвердил активацию. Точная причина показана в окне Epic.';
    return <div className="content"><section className="redeem-panel"><h2>{tr("Активировать ключ")}</h2>
    <p>{tr("Введите ключ в отдельном окне официального сайта Epic. Перед подтверждением проверьте название продукта и выбранный аккаунт.")}</p>
    <p>{tr(account.signedIn ? `В менеджере подключён ${account.displayName || 'аккаунт Epic'}. В окне активации должен быть выбран тот же аккаунт.` : 'При необходимости войдите в Epic в окне активации.')}</p>
    <div className="dialog-actions"><button className="primary" disabled={busy} onClick={open}>{tr(busy ? 'Окно активации открыто' : 'Открыть окно активации')}</button>
    <button onClick={onLibrary}><RefreshCw size={17}/>{tr("Открыть и обновить библиотеку")}</button></div>
    {tr(busy && !result && <p role="status">{tr("Ожидаем активацию в окне Epic…")}</p>)}
    {tr(result && <div className={`redemption-result redemption-${result.status}`} role="status">
      <h3>{tr(result.status === 'success' ? 'Ключ успешно активирован' : result.status === 'failed' ? 'Активация не удалась' : result.status === 'ready' ? 'Ключ проверен — ожидаем подтверждения' : 'Результат активации не подтверждён')}</h3>
      {tr(result.title && <p><b>{tr(result.status === 'success' ? 'Активировано:' : 'Продукт:')} {result.title}</b></p>)}
      {tr(result.status === 'success' && <p>{tr(result.title ? 'Библиотека обновляется в фоне.' : 'Epic подтвердил активацию, но не передал название продукта. Проверьте подтверждение в окне Epic.')}{tr(" Материалы Fab появятся после обновления; игры в этой библиотеке не отображаются.")}</p>)}
      {tr(result.status === 'failed' && <><p>{tr(reason)}</p>{tr(result.errorCode && <small>{tr("Код ошибки Epic: ")}{tr(result.errorCode)}</small>)}</>)}
      {tr(result.status === 'ready' && <p>{tr("Ключ ещё не активирован. Завершите подтверждение в окне Epic.")}</p>)}
      {tr(result.status === 'unknown' && <p>{tr("Окно закрыто, связь прервана или ответ Epic не удалось распознать. Проверьте результат у Epic и обновите библиотеку.")}</p>)}
    </div>)}
    <EpicAuthError error={error}/>
  </section></div>;
}
export function useEpic() {
    const [account, setAccount] = useState<Account>(signedOut);
    const [loginOpen, setLoginOpen] = useState(false);
    const [accountError, setAccountError] = useState('');
    const [accountBusy, setAccountBusy] = useState(false);
    useEffect(() => { let active = true; invoke<Account>('epic_status').then(a => { if (active)
        setAccount(a); }).catch(e => { if (active)
        setAccountError(String(e)); }); return () => { active = false; }; }, []);
    const openLogin = () => { setAccountError(''); setLoginOpen(true); };
    const logout = async () => {
        setAccountBusy(true);
        setAccountError('');
        try {
            await invoke('epic_logout');
            setAccount(signedOut);
        }
        catch (e) {
            setAccountError(String(e));
        }
        finally {
            setAccountBusy(false);
        }
    };
    return { account, accountError, accountBusy, openLogin, logout,
        dialog: loginOpen ? <LoginDialog onClose={() => setLoginOpen(false)} onSuccess={a => { setAccount(a); setLoginOpen(false); }}/> : null };
}
function LoginDialog({ onClose, onSuccess }: {
    onClose: () => void;
    onSuccess: (a: Account) => void;
}) {
    const [remember, setRemember] = useState(true);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const begin = async () => {
        setError('');
        setBusy(true);
        try {
            onSuccess(await invoke<Account>('epic_begin_login', { remember }));
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
        }
    };
    const cancel = async () => {
        try {
            await invoke('epic_cancel_login');
        }
        catch (e) {
            setError(String(e));
        }
    };
    return <div className="modal-backdrop"><section className="login-dialog" role="dialog" aria-modal="true" aria-labelledby="login-title">
    <div className="section-head"><h2 id="login-title">{tr("Вход в Epic Games")}</h2><button aria-label={tr("Закрыть вход")} disabled={busy} onClick={onClose}><X size={18}/></button></div>
    <p>{tr("Откроется отдельное окно официального сайта Epic Games. Войдите в свой аккаунт — после подтверждения менеджер подключит библиотеку автоматически.")}</p>
    <label className="remember"><input type="checkbox" checked={remember} disabled={busy} onChange={e => setRemember(e.target.checked)}/>{tr(" Запомнить вход на этом компьютере")}</label>
    <p className="muted">{tr("Пароль и код подтверждения вводятся только на сайте Epic. Сохранённый сеанс защищён Windows.")}</p>
    {tr(busy && <div className="library-loading" role="status"><RefreshCw className="loading-spinner" size={24}/><div><strong>{tr("Ожидаем вход в Epic Games…")}</strong><p>{tr("Завершите вход в открывшемся окне. Копировать коды не нужно.")}</p></div></div>)}
    <EpicAuthError error={error}/>
    <div className="dialog-actions"><button className="primary" onClick={begin} disabled={busy}><LogIn size={17}/>{tr(busy ? 'Выполняется вход…' : 'Войти через Epic Games')}</button>
    {tr(busy && <button onClick={cancel}>{tr("Отменить вход")}</button>)}</div>
  </section></div>;
}
const coverMemory = new Map<string, string>();
export function FabCover({ item }: {
    item: FabItem;
}) {
    const key = `${item.id}:${item.cover}:${item.coverRevision || 0}`;
    const [src, setSrc] = useState(() => coverMemory.get(key) || '');
    const ref = useRef<HTMLSpanElement>(null);
    useEffect(() => {
        let active = true;
        setSrc(coverMemory.get(key) || '');
        if (coverMemory.has(key) || !item.cover || !ref.current)
            return;
        const observer = new IntersectionObserver(entries => {
            if (!entries.some(e => e.isIntersecting))
                return;
            observer.disconnect();
            invoke<string>('fab_cover', { id: item.id, revision: item.coverRevision || 0 }).then(value => { coverMemory.set(key, value); if (active)
                setSrc(value); }).catch(() => { });
        }, { rootMargin: '150px' });
        observer.observe(ref.current);
        return () => { active = false; observer.disconnect(); };
    }, [item.id, item.cover, item.coverRevision, key]);
    return <span ref={ref} className="fab-cover">{tr(src ? <img src={src} alt={tr("")} loading="lazy"/> : <PackageOpen size={36} aria-hidden="true"/>)}</span>;
}
export function FabLibrary({ account, onLogin, onDownload, downloadBusy, downloadProgress, refreshSignal, downloaded, onVaultChanged, pluginRequest = 0,pendingProgress={},onResume }: {
    pendingProgress?:Record<string,{id:string;percent:number;phase:string;taskId:string}>;
    onResume?:(id:string)=>void;
    pluginRequest?: number;
    account: Account;
    onLogin: () => void;
    onDownload: (item: FabItem, install?: boolean, downloadOnly?: boolean) => void;
    downloadBusy: boolean;
    downloadProgress: {
        id: string;
        percent: number;
        phase: string;
    } | null;
    refreshSignal: number;
    onVaultChanged: () => Promise<void>;
    downloaded: {
        id: string;
        assetId?: string;
        title: string;
    }[];
}) {
    const [items, setItems] = useState<Item[]>([]);
    const [loading, setLoading] = useState(false);
    const [loaded, setLoaded] = useState(false);
    const [error, setError] = useState('');
    const [query, setQuery] = useState('');
    const [kind, setKind] = useState('All');
    const [topic,setTopic]=useState('');
    useEffect(() => { if (pluginRequest) {
        setKind('Plugin');
        setPage(0);
        setQuery('');
        setVersion('All');
        setCategory('all');
        setTopic('');
    } }, [pluginRequest]);
    const [version, setVersion] = useState('All');
    const [page, setPage] = useState(0);
    const [revision, setRevision] = useState(0);
    const [category, setCategory] = useState<'all' | 'downloaded'>('all');
    const [pageSize, setPageSize] = useState(() => { try {
        const value = Number(getPreference('um-library-page-size'));
        return [24, 48, 96, 192].includes(value) ? value : 48;
    }
    catch {
        return 48;
    } });
    const [progress, setProgress] = useState({ pages: 0, items: 0 });
    const [cachedAt, setCachedAt] = useState(0);
    const lastRequest = useRef<{
        account: string;
        revision: number;
        signal: number;
    } | null>(null);
    useEffect(() => { setItems([]); setLoaded(false); setPage(0); setCachedAt(0); setTopic(''); }, [account.signedIn, account.displayName]);
    useEffect(() => {
        let active = true;
        setError('');
        setProgress({ pages: 0, items: 0 });
        if (!account.signedIn) {
            setLoading(false);
            lastRequest.current = null;
            return;
        }
        const previous = lastRequest.current;
        const forced = previous?.account === account.displayName && (previous.revision !== revision || previous.signal !== refreshSignal);
        lastRequest.current = { account: account.displayName || '', revision, signal: refreshSignal };
        setLoading(true);
        (async () => {
            if (!forced) {
                const cache = await invoke<{
                    items: Item[];
                    cachedAt: number;
                    stale: boolean;
                } | null>('fab_cached_library').catch(() => null);
                if (!active)
                    return;
                if (cache) {
                    setItems(cache.items);
                    setLoaded(true);
                    setCachedAt(cache.cachedAt);
                    if (!cache.stale)
                        return;
                }
            }
            const onProgress = new Channel<{
                pages: number;
                items: number;
            }>(value => { if (active)
                setProgress(value); });
            const result = await invoke<Library>('fab_library', { onProgress });
            if (active) {
                setItems(result.items.map(item => ({ ...item, coverRevision: Date.now() })));
                setLoaded(true);
                setCachedAt(Math.floor(Date.now() / 1000));
            }
        })().catch(e => { if (active)
            setError(String(e)); }).finally(() => { if (active)
            setLoading(false); });
        return () => { active = false; };
    }, [account.signedIn, account.displayName, revision, refreshSignal]);
    useEffect(() => { setPage(0); }, [query, kind, version, category, topic, pageSize]);
    const downloadedIds = useMemo(() => {
        const titles = new Map<string, number>();
        for (const item of items)
            titles.set(item.title, (titles.get(item.title) || 0) + 1);
        return new Set(items.filter(i => downloaded.some(p => p.assetId === i.id || (!p.assetId && p.title === i.title && titles.get(i.title) === 1))).map(i => i.id));
    }, [items, downloaded]);
    const versions = useMemo(() => [...new Set(items.flatMap(i => i.engineVersions))].sort().reverse(), [items]);
    const topics=useMemo(()=>libraryTopics(items).sort((a,b)=>String(tr(a.label)).localeCompare(String(tr(b.label)),getLocale())),[items,getLocale()]);
    const filtered = useMemo(() => items.filter(i => (category === 'all' || downloadedIds.has(i.id)) && (!topic || i.categories.some(c=>categoryKey(c)===topic)) && (kind === 'All' || i.kind === kind) && (version === 'All' || i.engineVersions.includes(version)) && `${i.title} ${i.categories.join(' ')}`.toLowerCase().includes(query.toLowerCase())), [items, query, kind, version, category, topic, downloadedIds]);
    const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
    const currentPage = Math.min(page, pages - 1);
    if (!account.signedIn)
        return <div className="content"><div className="library-empty"><PackageOpen size={44}/><h2>{tr("Ваша библиотека Fab")}</h2><p>{tr("Войдите в Epic Games, чтобы увидеть приобретённые материалы.")}</p><button className="primary" onClick={onLogin}>{tr("Войти в Epic")}</button></div></div>;
    return <div className="content library-content">
    <div className="library-summary"><div><span className="muted">{tr(items.length)}{tr(" материалов")}{tr(cachedAt ? ` · Синхронизация: ${new Date(cachedAt * 1000).toLocaleString(getLocale())}` : '')}</span></div><PageActions><button disabled={loading} onClick={() => setRevision(r => r + 1)}><RefreshCw size={17}/>{tr(loading ? 'Синхронизация…' : 'Обновить библиотеку')}</button></PageActions></div>
    <label className="downloaded-filter"><input type="checkbox" checked={category === 'downloaded'} onChange={e => setCategory(e.target.checked ? 'downloaded' : 'all')}/>{tr("Показать только скачанное ")}<span>{tr("(")}{tr(downloadedIds.size)}{tr(")")}</span></label>
    {tr(loading && <div className="library-loading" role="status"><RefreshCw className="loading-spinner" size={24}/><div><strong>{tr(loaded ? 'Обновляем список в фоне…' : 'Загружаем библиотеку…')}</strong><p>{tr(progress.pages ? `Получено материалов: ${progress.items} · Страниц: ${progress.pages}` : 'Проверяем локальный список и подключение…')}</p></div></div>)}
    <div className="library-filters"><input aria-label={tr("Поиск в Fab")} placeholder={tr("Название или категория")} value={query} onChange={e => setQuery(e.target.value)}/><select aria-label={tr("Тип материала")} value={kind} onChange={e => setKind(e.target.value)}><option value="All">{tr("Все типы")}</option><option value="Asset">{tr("Ассеты")}</option><option value="Plugin">{tr("Плагины")}</option><option value="Project">{tr("Проекты")}</option></select><select aria-label={tr("Тематика")} value={topic} onChange={e=>setTopic(e.target.value)}><option value="">{tr("Все тематики")}</option>{topics.map(t=><option key={t.key} value={t.key}>{tr(t.label)}</option>)}</select><select aria-label={tr("Версия Unreal Engine")} value={version} onChange={e => setVersion(e.target.value)}><option value="All">{tr("Все версии UE")}</option>{tr(versions.map(v => <option key={v}>{tr(v)}</option>))}</select><label className="page-size">{tr("На странице")}<select aria-label={tr("Материалов на странице")} value={pageSize} onChange={e => { const n = Number(e.target.value); setPageSize(n); try {
        setPreference('um-library-page-size', String(n));
    }
    catch { } }}>{tr([24, 48, 96, 192].map(n => <option key={n} value={n}>{tr(n)}</option>))}</select></label></div>
    <EpicAuthError error={error}/>
    {tr(!loading && loaded && !filtered.length && <div className="empty">{tr(category === 'downloaded' ? 'В Vault пока нет материалов, соответствующих фильтрам.' : 'Материалы не найдены.')}</div>)}
    <div className="library-items"><div className="fab-grid">{tr(filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize).map(item => <LibraryTile key={item.id} item={item} onOpen={onDownload} busy={downloadBusy} progress={downloadProgress?.id===item.id?downloadProgress:pendingProgress[item.id]||null} resumeId={pendingProgress[item.id]?.taskId} onResume={onResume} downloaded={downloadedIds.has(item.id)} onVaultChanged={onVaultChanged} packageIds={downloaded.filter(p => p.assetId === item.id || (!p.assetId && p.title === item.title && downloadedIds.has(item.id))).map(p => p.id)} packageId={downloaded.find(p => p.assetId === item.id || (!p.assetId && p.title === item.title && downloadedIds.has(item.id)))?.id}/>))}</div></div>
    {tr(filtered.length > 0 && <div className="pagination"><span>{tr("Найдено: ")}{tr(filtered.length)}{tr(" · Страница ")}{tr(currentPage + 1)}{tr(" из ")}{tr(pages)}</span><button disabled={!currentPage} onClick={() => setPage(p => p - 1)}>{tr("Назад")}</button><button disabled={currentPage >= pages - 1} onClick={() => setPage(p => p + 1)}>{tr("Далее")}</button></div>)}
  </div>;
}
