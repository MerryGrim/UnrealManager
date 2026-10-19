import { tr, getLocale } from './i18n';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { RefreshCw, Download, CheckCircle2, AlertCircle, PackageOpen } from 'lucide-react';
import { PageActions } from './PageActions';
import { FabCover, type FabItem } from './EpicLibrary';
import type { VaultPackage, UpdateTransfer } from './Vault';
import { updateChoices } from './lib/updateChoices';
import { engineVersion } from './lib/materialVersions';
type Check = {
    currentId: string;
    latestId: string;
    buildVersion: string;
    changed: number;
    added: number;
    removed: number;
    bytes: number;
    downloadBytes: number;
    status: 'current' | 'available' | 'downloaded';
};
type RowResult = {
    check?: Check;
    error?: string;
    checkedAt: number;
    choice: string;
};
const size = (n: number) => n >= 1073741824 ? `${(n / 1073741824).toFixed(2)} ГБ` : `${(n / 1048576).toFixed(1)} МБ`;
export function UpdatesView({ items, account, busy, onDownload, onCount, onProjectUpdate, transfers }: {
    transfers: Record<string, UpdateTransfer>;
    items: VaultPackage[];
    account: {
        signedIn: boolean;
        displayName: string | null;
    };
    busy: boolean;
    onDownload: (item: FabItem, artifact: string, engine: string, currentId: string) => void;
    onCount: (count: number) => void;
    onProjectUpdate: (item?: VaultPackage, newId?: string) => void;
}) {
    const [library, setLibrary] = useState<FabItem[]>([]), [results, setResults] = useState<Record<string, RowResult>>({});
    const [choices, setChoices] = useState<Record<string, string>>({}), [running, setRunning] = useState(false), [stopping, setStopping] = useState(false);
    const [progress, setProgress] = useState(''), [error, setError] = useState(''), [checkedAt, setCheckedAt] = useState(0);
    const generation = useRef(0), stop = useRef(false), lock = useRef(false);
    useEffect(() => {
        const current = ++generation.current;
        stop.current = true;
        setLibrary([]);
        setResults({});
        setChoices({});
        setCheckedAt(0);
        setError('');
        if (account.signedIn)
            invoke<{
                items: FabItem[];
            } | null>('fab_cached_library').then(cache => { if (current === generation.current)
                setLibrary(cache?.items || []); }).catch(() => { });
        return () => { generation.current++; stop.current = true; };
    }, [account.signedIn, account.displayName]);
    const rows = useMemo(() => items.map(local => {
        const item = library.find(i => !!local.assetId && i.id === local.assetId);
        const options = item ? updateChoices(local, item) : { choices: [], defaultKey: '' };
        const choice = choices[local.id] ?? options.defaultKey;
        const selected = options.choices.find(v => v.key === choice);
        const result = results[local.id]?.choice === choice ? results[local.id] : undefined;
        const check = result?.check;
        const status = check?.status === 'available' && items.some(p => p.id === check.latestId) ? 'downloaded' : check?.status;
        const raw = transfers[local.id];
        const transfer = raw?.state === 'done' && check && raw.newId !== check.latestId ? undefined : raw;
        return { local, item, options, choice, selected, result, status, transfer };
    }), [items, library, choices, results, transfers]);
    const visible = rows.filter(r => r.status === 'available' || r.status === 'downloaded' || r.transfer);
    const issues = rows.filter(r => r.result?.error || !r.selected || !r.item);
    const available = rows.filter(r => r.status === 'available' && r.transfer?.state !== 'done').length;
    useEffect(() => onCount(available), [available, onCount]);
    const checkUpdates = async () => {
        if (lock.current || busy || !account.signedIn)
            return;
        lock.current = true;
        stop.current = false;
        setStopping(false);
        setRunning(true);
        setError('');
        setProgress('Обновляем сведения Fab…');
        const current = generation.current;
        try {
            const onProgress = new Channel<{
                pages: number;
                items: number;
            }>(p => { if (current === generation.current)
                setProgress(`Сведения Fab: ${p.items} материалов`); });
            const fresh = await invoke<{
                items: FabItem[];
            }>('fab_library', { onProgress });
            if (current !== generation.current)
                return;
            setLibrary(fresh.items);
            let checked = 0;
            for (const local of items) {
                if (stop.current || current !== generation.current)
                    break;
                const item = fresh.items.find(i => !!local.assetId && i.id === local.assetId);
                if (!item)
                    continue;
                const options = updateChoices(local, item), key = choices[local.id] ?? options.defaultKey, choice = options.choices.find(v => v.key === key);
                if (!choice)
                    continue;
                setProgress(`Проверяем ${++checked} из ${items.length}: ${local.title}`);
                let result: RowResult;
                try {
                    const check = await invoke<Check>('vault_check_update', { id: local.id, assetId: item.id, artifact: choice.artifactId, engineVersion: choice.engine });
                    result = { check, choice: key, checkedAt: Date.now() };
                }
                catch (e) {
                    result = { error: String(e), choice: key, checkedAt: Date.now() };
                }
                if (current === generation.current)
                    setResults(old => ({ ...old, [local.id]: result }));
            }
            if (current === generation.current)
                setCheckedAt(Date.now());
        }
        catch (e) {
            if (current === generation.current)
                setError(String(e));
        }
        finally {
            lock.current = false;
            setRunning(false);
            setStopping(false);
            setProgress('');
        }
    };
    return <div className="content updates-content">
  <PageActions><button disabled={running || busy || !account.signedIn || !items.length} onClick={() => void checkUpdates()}><RefreshCw size={17} className={running ? 'spin' : ''}/>{tr(running ? 'Проверяем…' : 'Проверить обновления')}</button></PageActions>
  <div className="updates-intro"><div><h3>{tr("Доступные обновления")}</h3><p>{tr("Обновите материал в Vault одним нажатием. Заменить файлы в проекте можно отдельно, после загрузки.")}</p></div><strong>{tr(available)}<small>{tr("доступно")}</small></strong></div>
  {tr(!account.signedIn && <p className="error-text">{tr("Для проверки войдите в Epic через меню аккаунта.")}</p>)}
  {tr(error && <p role="alert" className="error-text">{tr(error)}</p>)}
  {tr(running && <div className="updates-running" role="status"><RefreshCw size={19} className="spin"/><span>{tr(progress)}</span><button disabled={stopping} onClick={() => { stop.current = true; setStopping(true); }}>{tr(stopping ? 'Завершаем текущий запрос…' : 'Остановить проверку')}</button></div>)}
  <p className="muted">{tr(checkedAt ? `Проверка: ${new Date(checkedAt).toLocaleString(getLocale())}` : 'Нажмите «Проверить обновления», чтобы найти новые версии.')}</p>
  <div className="updates-list">{tr(visible.map(({ local, item, selected, result, status, transfer }) => {
            const active = transfer?.state === 'preparing' || transfer?.state === 'downloading';
            const ready = transfer?.state === 'done' || status === 'downloaded';
            const newId = transfer?.newId || result?.check?.latestId;
            return <article className="update-card" key={local.id}>
    <div className="update-card-heading"><div className="transfer-art">{tr(item ? <FabCover item={item}/> : <PackageOpen size={26}/>)}</div><div className="update-title"><h3>{local.title}</h3><p>{tr({ Asset: 'Ассет', Project: 'Проект', Plugin: 'Плагин' }[local.kind] || local.kind)}{tr(" · UE ")}{tr(selected?.engine || local.engines.map(engineVersion).join(', '))}</p></div><span className="update-status">{tr(active ? <><RefreshCw size={16} className="spin"/>{tr("Обновляется")}</> : transfer?.state === 'error' ? <><AlertCircle size={16}/>{tr("Не удалось обновить")}</> : ready ? <><CheckCircle2 size={16}/>{tr("Обновлено в Vault")}</> : 'Доступно обновление')}</span></div>
    {tr(result?.check && !ready && <div className="update-summary"><span>{tr("Изменено: ")}<b>{tr(result.check.changed)}</b></span><span>{tr("Добавлено: ")}<b>{tr(result.check.added)}</b></span><span>{tr("Загрузка: ")}<b>{tr("до ")}{tr(size(result.check.downloadBytes))}</b></span></div>)}
    {tr(active && <div className="update-transfer" role="status"><progress max={100} value={transfer.percent ?? undefined}/><span>{tr(transfer.phase)}{tr(transfer.percent !== null ? ` · ${transfer.percent}%` : '')}</span></div>)}
    {tr(transfer?.state === 'error' && <p className="error-text" role="alert">{tr(transfer.phase)}</p>)}
    {tr(!active && <div className="update-footer"><span className="muted">{tr(ready ? 'Копия готова. Файлы проектов пока не изменены.' : 'Старая копия сохранится.')}</span>{tr(ready ? <button disabled={busy || running || !items.some(p => p.id === newId)} onClick={() => onProjectUpdate(local, newId)}><PackageOpen size={17}/>{tr(local.kind === 'Plugin' ? 'Обновить установленный плагин…' : 'Обновить в проекте…')}</button> : item && selected && <button className="primary" disabled={busy || running} onClick={() => onDownload(item, selected.artifactId, selected.engine, local.id)}><Download size={17}/>{tr(transfer?.state === 'error' ? 'Повторить' : 'Обновить')}</button>)}</div>)}
   </article>;
        }))}</div>
  {tr(!visible.length && !running && <div className="downloads-empty">{tr(!items.length ? 'Сначала скачайте материалы или подключите Epic Vault в настройках.' : checkedAt ? 'Доступных обновлений не найдено.' : 'Здесь появятся только материалы, для которых найдено обновление.')}</div>)}
  {tr(!!checkedAt && issues.length > 0 && <details className="update-check-issues"><summary>{tr("Не удалось проверить: ")}{tr(issues.length)}</summary>{tr(issues.map(({ local, item, options, choice, result }) => <div key={local.id}><b>{local.title}</b><p className="muted">{tr(result?.error || (!item ? 'Нет соответствия в библиотеке Fab.' : !options.choices.length ? 'Нет пакета для исходной версии UE.' : 'Уточните версию для проверки.'))}</p>{tr(item && options.choices.length > 0 && <select aria-label={tr(`Версия UE: ${local.title}`)} disabled={busy || running} value={choice} onChange={e => setChoices(old => ({ ...old, [local.id]: e.target.value }))}><option value="">{tr("Выберите версию")}</option>{tr(options.choices.map(v => <option key={v.key} value={v.key}>{tr("UE ")}{tr(v.engine)}{tr(" · ")}{tr(v.artifactId)}</option>))}</select>)}</div>))}<p className="muted">{tr("После выбора версии повторите проверку.")}</p></details>)}
 </div>;
}
