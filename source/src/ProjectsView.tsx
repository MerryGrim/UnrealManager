import { Channel, invoke } from '@tauri-apps/api/core';
import { FolderSearch, RefreshCw, Search, Square, X } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { getLocale, tr } from './i18n';
import { PageActions } from './PageActions';
import { ProjectTile } from './ProjectTile';
import type { UnrealProject } from './lib/backend';

export type DiscoveryProgress = {
    id: string;
    directories: number;
    scannedEntries: number;
    found: number;
    skipped: number;
    currentDirectory: string;
    phase: 'scan' | 'save' | 'done' | 'cancelled';
};

export type DiscoveryResult = {
    id: string;
    root: string;
    directories: number;
    scannedEntries: number;
    found: number;
    added: number;
    skipped: number;
    cancelled: boolean;
    limited: boolean;
    limitReason: 'depth' | 'entries' | 'time' | 'projects' | null;
    errors: string[];
};

type Work = 'refresh' | 'pick' | 'scan' | null;

export function ProjectsView({ projects, onRefresh, initialLoading, onRemoved }: {
    projects: UnrealProject[];
    onRefresh: () => Promise<UnrealProject[]>;
    initialLoading: boolean;
    onRemoved: (path: string) => void;
}) {
    const [query, setQuery] = useState('');
    const [work, setWork] = useState<Work>(null);
    const [progress, setProgress] = useState<DiscoveryProgress | null>(null);
    const [result, setResult] = useState<DiscoveryResult | null>(null);
    const [refreshedCount, setRefreshedCount] = useState<number | null>(null);
    const [error, setError] = useState('');
    const [listRefreshFailed, setListRefreshFailed] = useState(false);
    const [launchError, setLaunchError] = useState('');
    const [stopping, setStopping] = useState(false);
    const lock = useRef(false);
    const activeScan = useRef<string | null>(null);
    const mounted = useRef(true);
    const cancelLock = useRef(false);
    const busy = work !== null || initialLoading;
    const filtered = useMemo(() => projects.filter(project => project.name.toLowerCase().includes(query.toLowerCase())), [projects, query]);
    const projectCards = useMemo(() => filtered.map(project => <ProjectTile key={project.path} project={project} onError={setLaunchError} onRemoved={onRemoved}/>), [filtered, onRemoved, getLocale()]);
    const number = (value: number) => new Intl.NumberFormat(getLocale()).format(value);

    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            const id = activeScan.current;
            activeScan.current = null;
            // A real unmount stops traversal; switching sections keeps this view mounted.
            if (id) void invoke('project_discovery_cancel', { id }).catch(() => {});
        };
    }, []);

    const refresh = async () => {
        if (lock.current || initialLoading) return;
        lock.current = true;
        setWork('refresh');
        setError('');
        setListRefreshFailed(false);
        setLaunchError('');
        setProgress(null);
        setResult(null);
        setRefreshedCount(null);
        try {
            const list = await onRefresh();
            if (mounted.current) setRefreshedCount(list.length);
        } catch (failure) {
            if (mounted.current) setError(String(failure));
        } finally {
            lock.current = false;
            if (mounted.current) setWork(null);
        }
    };

    const discover = async () => {
        if (lock.current || initialLoading) return;
        lock.current = true;
        setWork('pick');
        setError('');
        setListRefreshFailed(false);
        setLaunchError('');
        try {
            const root = await invoke<string | null>('choose_project_location', { folder: true });
            if (!root || !mounted.current) return;
            const id = crypto.randomUUID();
            activeScan.current = id;
            setWork('scan');
            setStopping(false);
            setResult(null);
            setRefreshedCount(null);
            setProgress({ id, directories: 0, scannedEntries: 0, found: 0, skipped: 0, currentDirectory: root, phase: 'scan' });
            const onProgress = new Channel<DiscoveryProgress>();
            onProgress.onmessage = value => {
                if (mounted.current && activeScan.current === value.id) setProgress(value);
            };
            const completed = await invoke<DiscoveryResult>('project_discovery_scan', { id, root, onProgress });
            if (!mounted.current || activeScan.current !== id) return;
            activeScan.current = null;
            setResult(completed);
            setStopping(false);
            // The worker atomically records discovered descriptors, including partial results.
            // Reload the existing catalogue; do not replace it with only this scan's findings.
            try {
                await onRefresh();
            } catch (failure) {
                if (mounted.current) {
                    setListRefreshFailed(true);
                    setError(String(failure));
                }
            }
        } catch (failure) {
            if (mounted.current) setError(String(failure));
        } finally {
            activeScan.current = null;
            lock.current = false;
            cancelLock.current = false;
            if (mounted.current) {
                setWork(null);
                setStopping(false);
            }
        }
    };

    const stop = async () => {
        const id = activeScan.current;
        if (!id || cancelLock.current || progress?.phase === 'save') return;
        cancelLock.current = true;
        setStopping(true);
        try {
            const accepted = await invoke<boolean>('project_discovery_cancel', { id });
            if (mounted.current && activeScan.current === id && !accepted) setStopping(false);
        } catch (failure) {
            if (mounted.current && activeScan.current === id) {
                setError(String(failure));
                setStopping(false);
            }
        } finally {
            cancelLock.current = false;
        }
    };

    const status = work === 'pick' ? 'Выбираем папку…'
        : work === 'refresh' || (work === 'scan' && result) || (initialLoading && work === null) ? 'Обновляем список проектов…'
        : work === 'scan' && progress?.phase === 'save' ? 'Сохраняем найденные проекты…'
        : work === 'scan' && stopping ? 'Останавливаем поиск…'
        : work === 'scan' && !result ? 'Ищем проекты…'
        : result?.cancelled ? 'Поиск остановлен. Найденные проекты сохранены.'
        : result ? 'Поиск завершён.'
        : refreshedCount !== null ? 'Список проектов обновлён.'
        : progress && error ? 'Поиск не завершён.' : '';
    const counts = result ?? progress;

    return <div className="content projects-content">
        <PageActions>
            <button disabled={busy} onClick={() => void refresh()}><RefreshCw size={17}/>{tr('Обновить список')}</button>
            <button className="primary" disabled={busy} onClick={() => void discover()}><FolderSearch size={17}/>{tr('Найти проекты')}</button>
        </PageActions>
        <div className="toolbar projects-toolbar">
            <label className="search"><Search size={17}/><input aria-label={tr('Поиск проектов')} placeholder={tr('Поиск проектов')} value={query} onChange={event => setQuery(event.target.value)}/></label>
            <span className="projects-visible-count">{tr('Показано проектов')}: <strong>{number(filtered.length)}</strong> / {number(projects.length)}</span>
            {query && <button className="projects-clear-filter" onClick={() => setQuery('')}><X size={15}/>{tr('Сбросить фильтр')}</button>}
        </div>
        <div className="projects-discovery-help">
            <p>{tr('Обновление проверяет стандартные папки и ранее найденные проекты. Для других расположений выберите «Найти проекты».')}</p>
            <p>{tr('Поиск выполняется только в выбранной папке или на диске. Его можно остановить; найденные проекты сохранятся в списке.')}</p>
        </div>
        {status && <section className="projects-discovery-status" aria-label={tr('Поиск проектов')}>
            <div className="projects-discovery-heading">
                <strong role="status" aria-live="polite" aria-atomic="true">{tr(status)}</strong>
                {work === 'scan' && activeScan.current && <button disabled={stopping || progress?.phase === 'save'} onClick={() => void stop()}><Square size={15}/>{tr(stopping ? 'Останавливаем поиск…' : 'Остановить поиск')}</button>}
            </div>
            {counts && <dl className="projects-discovery-counts">
                <div><dt>{tr('Проверено папок')}</dt><dd>{number(counts.directories)}</dd></div>
                <div><dt>{tr('Найдено проектов')}</dt><dd>{number(counts.found)}</dd></div>
                {result && <div><dt>{tr('Добавлено в список')}</dt><dd>{number(result.added)}</dd></div>}
                <div title={tr('Папки и записи, которые не удалось проверить или которые исключены из поиска.')}><dt>{tr('Пропущено')}</dt><dd>{number(counts.skipped)}</dd></div>
            </dl>}
            {refreshedCount !== null && <p className="projects-refresh-count">{tr('Найдено проектов')}: <strong>{number(refreshedCount)}</strong></p>}
            {work === 'scan' && !result && progress?.currentDirectory && <p className="projects-current-directory" title={progress.currentDirectory}><span>{tr('Текущая папка')}: </span><b>{progress.currentDirectory}</b></p>}
            {result?.limited && <p className="projects-scan-warning">{tr('Поиск ограничен. Можно выбрать более узкую папку.')}</p>}
            {!!result?.errors.length && <details className="projects-scan-errors"><summary>{tr('Проблемы при поиске')} ({number(result.errors.length)})</summary><ul>{result.errors.map((message, index) => <li key={index}>{tr(message)}</li>)}</ul></details>}
        </section>}
        {error && <div className="error-text projects-scan-error" role="alert">
            {listRefreshFailed && <p>{tr('Найденные проекты сохранены, но список не удалось обновить. Нажмите «Обновить список».')}</p>}
            <p>{tr(error)}</p>
        </div>}
        {launchError.trim() && <p className="error-text" role="alert">{tr(launchError)}</p>}
        <div className="fab-grid recent-projects">
            {projectCards}
        </div>
        {!filtered.length && !busy && <div className="empty">{tr('Проекты не найдены')}</div>}
    </div>;
}
