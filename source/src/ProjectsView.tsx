import { Channel, invoke } from '@tauri-apps/api/core';
import { FolderClosed, RefreshCw, Square, X } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { getLocale, tr } from './i18n';
import { PageActions } from './PageActions';
import { ProjectTile } from './ProjectTile';
import { ProjectScanFoldersDialog } from './ProjectScanFoldersDialog';
import type { UnrealProject } from './lib/backend';

export type DiscoveryProgress = {
    id: string; directories: number; scannedEntries: number; found: number;
    skipped: number; currentDirectory: string;
    phase: 'scan' | 'save' | 'done' | 'cancelled';
    limited?: boolean; limitReason?: 'depth' | 'entries' | 'time' | 'projects' | null; errors?: string[];
};
export type DiscoveryResult = {
    id: string; root: string; directories: number; scannedEntries: number;
    found: number; added: number; skipped: number; cancelled: boolean;
    limited: boolean; limitReason: 'depth' | 'entries' | 'time' | 'projects' | null; errors: string[];
};
export function ProjectsView({ projects, onProjects, initialLoading, onRemoved, active = true }: {
    projects: UnrealProject[];
    onProjects: (projects: UnrealProject[]) => void;
    initialLoading: boolean;
    onRemoved: (path: string) => void;
    active?: boolean;
}) {
    const [working, setWorking] = useState(false);
    const [progress, setProgress] = useState<DiscoveryProgress | null>(null);
    const [error, setError] = useState('');
    const [launchError, setLaunchError] = useState('');
    const [stopping, setStopping] = useState(false);
    const [foldersOpen, setFoldersOpen] = useState(false);
    const [notice, setNotice] = useState<{ text: string; id: number } | null>(null);
    const lock = useRef(false), activeScan = useRef<string | null>(null);
    const latestProgress = useRef<DiscoveryProgress | null>(null);
    const mounted = useRef(true), cancelLock = useRef(false), noticeId = useRef(0);
    const busy = working || initialLoading;
    const number = (value: number) => new Intl.NumberFormat(getLocale()).format(value);
    const projectCards = useMemo(() => projects.map(project => <ProjectTile key={project.path} project={project} onError={setLaunchError} onRemoved={onRemoved}/>), [projects, onRemoved, getLocale()]);
    const notify = (text: string) => setNotice({ text, id: ++noticeId.current });
    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            const id = activeScan.current;
            activeScan.current = null;
            if (id) void invoke('project_discovery_cancel', { id }).catch(() => {});
        };
    }, []);
    useEffect(() => {
        if (!notice) return;
        const timer = window.setTimeout(() => setNotice(null), 5000);
        return () => window.clearTimeout(timer);
    }, [notice]);
    useEffect(() => { if (!active) { setFoldersOpen(false); setNotice(null); } }, [active]);
    const refresh = async () => {
        if (lock.current || initialLoading) return;
        lock.current = true;
        const id = crypto.randomUUID();
        activeScan.current = id;
        latestProgress.current = null;
        setWorking(true); setStopping(false); setError(''); setLaunchError(''); setProgress(null); setNotice(null);
        try {
            const onProgress = new Channel<DiscoveryProgress>();
            onProgress.onmessage = value => {
                if (mounted.current && activeScan.current === value.id) {
                    latestProgress.current = value; setProgress(value);
                }
            };
            const completed = await invoke<{ projects: UnrealProject[]; summary: DiscoveryResult }>('refresh_projects', { id, onProgress });
            if (!mounted.current || activeScan.current !== id) return;
            const last: DiscoveryProgress = { ...completed.summary, currentDirectory: completed.summary.root, phase: completed.summary.cancelled ? 'cancelled' : 'done' };
            latestProgress.current = last;
            setProgress(last);
            onProjects(completed.projects);
            if (!last.limited && !last.errors?.length) notify(`${tr(completed.summary.cancelled ? 'Поиск остановлен' : 'Список обновлён')} · ${tr('Найдено проектов')}: ${number(completed.projects.length)}`);
        } catch (failure) {
            if (mounted.current) setError(String(failure));
        } finally {
            activeScan.current = null; lock.current = false; cancelLock.current = false;
            if (mounted.current) { setWorking(false); setStopping(false); }
        }
    };
    const stop = async () => {
        const id = activeScan.current;
        if (!id || cancelLock.current || progress?.phase === 'save') return;
        cancelLock.current = true; setStopping(true);
        try {
            const accepted = await invoke<boolean>('project_discovery_cancel', { id });
            if (mounted.current && activeScan.current === id && !accepted) setStopping(false);
        } catch (failure) {
            if (mounted.current && activeScan.current === id) { setError(String(failure)); setStopping(false); }
        } finally { cancelLock.current = false; }
    };
    const warning = !!progress?.limited || !!progress?.errors?.length;
    const status = initialLoading && !working ? 'Обновляем список проектов…'
        : working && progress?.phase === 'save' ? 'Сохраняем найденные проекты…'
        : working && stopping ? 'Останавливаем поиск…'
        : working ? 'Ищем проекты…'
        : warning ? 'Не все папки удалось проверить.' : '';
    return <div className="content projects-content">
        <PageActions>
            <button disabled={busy} onClick={() => setFoldersOpen(true)}><FolderClosed size={17}/>{tr('Папки проектов')}</button>
            <button className="primary" disabled={busy} onClick={() => void refresh()}><RefreshCw size={17}/>{tr('Обновить список')}</button>
        </PageActions>
        {status && <section className="projects-discovery-status" aria-label={tr('Поиск проектов')}>
            <div className="projects-discovery-heading">
                <strong role="status" aria-live="polite" aria-atomic="true">{tr(status)}</strong>
                {working && <button disabled={stopping || progress?.phase === 'save'} onClick={() => void stop()}><Square size={15}/>{tr(stopping ? 'Останавливаем поиск…' : 'Остановить поиск')}</button>}
            </div>
            {progress && <dl className="projects-discovery-counts">
                <div><dt>{tr('Проверено папок')}</dt><dd>{number(progress.directories)}</dd></div>
                <div><dt>{tr('Найдено проектов')}</dt><dd>{number(progress.found)}</dd></div>
                <div><dt>{tr('Пропущено')}</dt><dd>{number(progress.skipped)}</dd></div>
            </dl>}
            {working && progress?.currentDirectory && <p className="projects-current-directory" title={progress.currentDirectory}><span>{tr('Текущая папка')}: </span><b>{progress.currentDirectory}</b></p>}
            {progress?.limited && <p className="projects-scan-warning">{tr('Поиск ограничен. Можно выбрать более узкую папку.')}</p>}
            {!!progress?.errors?.length && <details className="projects-scan-errors"><summary>{tr('Проблемы при поиске')} ({number(progress.errors.length)})</summary><ul>{progress.errors.map((message, index) => <li key={index}>{tr(message)}</li>)}</ul></details>}
        </section>}
        {error && <p className="error-text projects-scan-error" role="alert">{tr(error)}</p>}
        {launchError.trim() && <p className="error-text" role="alert">{tr(launchError)}</p>}
        <div className="fab-grid recent-projects">{projectCards}</div>
        {!projects.length && !busy && <div className="empty">{tr('Проекты не найдены')}</div>}
        {active && notice && <div className="projects-toast" key={notice.id}><span role="status" aria-live="polite">{notice.text}</span><button className="icon" aria-label={tr('Закрыть уведомление')} onClick={() => setNotice(null)}><X size={16}/></button></div>}
        {active && foldersOpen && <ProjectScanFoldersDialog onClose={() => setFoldersOpen(false)} onSaved={() => { setFoldersOpen(false); notify(tr('Папки проектов сохранены.')); }}/>}
    </div>;
}
