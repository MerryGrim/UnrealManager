import {getPreference,setPreference} from './preferences';
import {EpicAuthError} from './EpicAuthError';
import { tr, getLocale } from './i18n';
import { ProjectUpdateDialog } from './ProjectUpdateDialog';
import {ModalFocusScope} from './ModalFocusScope';
import type {EngineTransfer} from './EngineTransferCard';
import { DownloadsView } from './DownloadsView';
import {VaultPauseButton} from './VaultPauseButton';
import type {VaultTask} from './VaultResumeTasks';
import { historyKey, readDownloadHistory, seedDownloadHistory, recordDownload, clearDownloadHistory } from './DownloadHistory';
import { materialAction } from './LibraryTile';
import { engineVersion, materialVersions } from './lib/materialVersions';
import { useEffect, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {emptyVaultQueue, isManualQueueTransfer, type VaultQueueSnapshot} from './VaultQueue';
import { Download, FolderOpen, PackageOpen, RefreshCw, X } from 'lucide-react';
import { FabCover, type FabItem } from './EpicLibrary';
import type { EngineInstall, UnrealProject } from './lib/backend';
export type UpdateTransfer = {
    state: "preparing" | "downloading" | "done" | "error";
    phase: string;
    percent: number | null;
    newId?: string;
};
export type VaultPackage = {
    artifactId?: string;
    selectedEngine?: string;
    buildVersion?: string;
    source?: string;
    id: string;
    assetId?: string;
    title: string;
    kind: string;
    engines: string[];
    bytes: number;
    downloadBytes: number;
    files: number;
    path: string;
};
type Progress = {
    phase: string;
    done: number;
    total: number;
    filesDone: number;
    filesTotal: number;
    networkBytes: number;
    bytesPerSecond: number;
    paused?:boolean;
    canPause?:boolean;
};
type Preview = {
    projectFile?: string | null;
    target: string;
    files: number;
    conflicts: string[];
    existing: number;
    compatible: boolean;
};
const suggestedName = (title: string) => { const name = title.replace(/[^A-Za-z0-9_]/g, '').slice(0, 56); return /^[A-Za-z]/.test(name) ? name : `Project${name}`; };
const correctedKindMessage = 'Тип материала уточнён по файлам. Выберите место установки заново.';
const size = (n: number) => n >= 1073741824 ? `${(n / 1073741824).toFixed(2)} ГБ` : `${(n / 1048576).toFixed(1)} МБ`;
export function useVault(projects: UnrealProject[], onProjectsChanged: () => Promise<void> | void, engines: EngineInstall[] = [], onPlugins: () => void = () => { }, engineDownload?: {progress:EngineTransfer;onOpen:()=>void} | null) {
    const [updateRequest, setUpdateRequest] = useState<{
        item?: VaultPackage;
        newId?: string;
    } | null>(null);
    const openProjectUpdate = (item?: VaultPackage, newId?: string) => { if (!busy)
        setUpdateRequest({ item, newId }); };
    const [updateTransfers, setUpdateTransfers] = useState<Record<string, UpdateTransfer>>({});
    const updateLock = useRef(false);
    const [queue, setQueue] = useState<VaultQueueSnapshot>(emptyVaultQueue);
    const queueRef = useRef(emptyVaultQueue);
    const [queueBusy, setQueueBusy] = useState(false), [queueError, setQueueError] = useState(''), [queueVaultPath, setQueueVaultPath] = useState('');
    const queueLock = useRef(false), draftGeneration = useRef(0);
    const [items, setItems] = useState<VaultPackage[]>([]);
    const[pendingDownloads,setPendingDownloads]=useState<VaultTask[]>([]);
    const[restoringDownload,setRestoringDownload]=useState(false);
    const transferLock=useRef(false);
    const refreshPending=async()=>{try{setPendingDownloads(await invoke<VaultTask[]>('vault_resume_tasks')||[]);}catch(e){setError(String(e));}};
    const [history, setHistory] = useState(readDownloadHistory);
    const [historyError, setHistoryError] = useState('');
    useEffect(() => { if (!history.initialized)
        return; try {
        setPreference(historyKey, JSON.stringify(history));
        setHistoryError('');
    }
    catch {
        setHistoryError('Не удалось сохранить список загрузок. После перезапуска изменения списка могут быть потеряны.');
    } }, [history]);
    const [epicDirectory, setEpicDirectory] = useState('');
    const [epicResult, setEpicResult] = useState<{
        directory: string;
        items: VaultPackage[];
        warnings: string[];
    } | null>(null);
    const [epicScan, setEpicScan] = useState<{
        done: number;
        total: number;
    } | null>(null);
    useEffect(() => { invoke<{
        directory: string;
        items: VaultPackage[];
        warnings: string[];
    }>('vault_epic_status').then(result => { setEpicDirectory(result.directory); setEpicResult(result); }).catch(() => { }); }, []);
    const [selected, setSelected] = useState<FabItem | null>(null);
    const [artifact, setArtifact] = useState('');
    const [projectEngine, setProjectEngine] = useState('');
    const [plan, setPlan] = useState<VaultPackage | null>(null);
    const preparedDraft = useRef<{assetId: string; artifact: string; plan: VaultPackage} | null>(null);
    const [manualBusy, setBusy] = useState(false);
    const busy = manualBusy || !!queue.activeId || !!engineDownload;
    const [progress, setProgress] = useState<Progress | null>(null);
    const [error, setError] = useState('');
    const [message, setMessage] = useState('');
    const [pluginChoice, setPluginChoice] = useState(false);
    const [pluginDestination, setPluginDestination] = useState<'project' | 'engine'>('project');
    const [installItem, setInstallItem] = useState<VaultPackage | null>(null);
    const [target, setTarget] = useState('');
    const [projectName, setProjectName] = useState('');
    const [preview, setPreview] = useState<Preview | null>(null);
    const [acceptVersion, setAcceptVersion] = useState(false);
    const [mode, setMode] = useState<'vault' | 'install' | null>(null);
    const [activeDownload, setActiveDownload] = useState<{
        item: FabItem;
        plan: VaultPackage;
        artifact: string;
        projectEngine: string;
        target: string;
        projectName: string;
        mode: 'vault' | 'install' | null;
    } | null>(null);
    const [installing, setInstalling] = useState(false);
    const [picking, setPicking] = useState(false);
    const discardPrepared = () => {
        draftGeneration.current++;
        preparedDraft.current = null;
        if (!activeDownload && !queue.activeId) void invoke('vault_discard').catch(() => {});
        setPlan(null);
        setPreview(null);
    };
    const closeSelected = () => {
        if (!activeDownload && !queue.activeId) discardPrepared();
        setSelected(null);
    };
    const browse = async (kind: string, title: string) => {
        setPicking(true);
        setError('');
        try {
            const directory = await invoke<string | null>('choose_project_location', { folder: kind === 'Project' });
            if (directory) {
                setTarget(kind === 'Project' ? `${directory.replace(/[\\/]+$/, '')}\\${projectName || suggestedName(title)}` : directory);
                setPlan(null);
                setPreview(null);
                setAcceptVersion(false);
            }
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setPicking(false);
        }
    };
    const refresh = async () => { try {
        const listed = await invoke<VaultPackage[]>('vault_list');
        setItems(listed);
        setHistory(current => seedDownloadHistory(current, listed));
    }
    catch (e) {
        setError(String(e));
    } };
    const acceptQueue = (next: VaultQueueSnapshot) => {
        if (!next || next.revision < queueRef.current.revision) return;
        const metadataChanged = next.revision !== queueRef.current.revision;
        queueRef.current = {...next, completed: null};
        setQueue(queueRef.current);
        if (next.completed) {
            setHistory(current => recordDownload(current, next.completed!));
            void refresh();
            void refreshPending();
        }
        else if (metadataChanged && next.items.some(item => item.packageId && (item.status === 'interrupted' || item.status === 'needsAction'))) void refreshPending();
    };
    useEffect(() => {
        let alive = true;
        const accept = (next: VaultQueueSnapshot) => {if (alive) acceptQueue(next);};
        const subscription = listen<VaultQueueSnapshot>('vault-queue-changed', event => accept(event.payload));
        void subscription.catch(e => {if (alive) setQueueError(String(e));});
        void invoke<VaultQueueSnapshot>('vault_queue_get').then(accept).catch(e => {if (alive) setQueueError(String(e));});
        return () => {alive = false; void subscription.then(unlisten => unlisten()).catch(() => {});};
    }, []);
    const queueCommand = async (name: string, args: Record<string, unknown>) => {
        if (queueLock.current) return false;
        queueLock.current = true; setQueueBusy(true); setQueueError('');
        try {acceptQueue(await invoke<VaultQueueSnapshot>(name, args)); return true;}
        catch (e) {
            setQueueError(String(e));
            try {acceptQueue(await invoke<VaultQueueSnapshot>('vault_queue_get'));} catch {}
            return false;
        } finally {queueLock.current = false; setQueueBusy(false);}
    };
    useEffect(() => { void refresh();void refreshPending(); }, []);
    const syncEpic = async () => {
        setBusy(true);
        setError('');
        setEpicScan({ done: 0, total: 0 });
        try {
            const onProgress = new Channel<{
                done: number;
                total: number;
            }>(setEpicScan);
            const result = await invoke<{
                directory: string;
                items: VaultPackage[];
                warnings: string[];
            }>('vault_sync_epic', { directory: epicDirectory, onProgress });
            setEpicResult(result);
            await refresh();
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
            setEpicScan(null);
        }
    };
    const select = (item: FabItem, installNow = false, downloadOnly = false) => {
        if (activeDownload && activeDownload.item.id === item.id) {
            setSelected(activeDownload.item);
            setInstallItem(null);
            const same = activeDownload.item.id === item.id;
            setProjectEngine(same ? activeDownload.projectEngine : '');
            setPlan(same ? activeDownload.plan : null);
            setArtifact(same ? activeDownload.artifact : '');
            setProjectName(same ? activeDownload.projectName : suggestedName(item.title));
            setMode(same ? activeDownload.mode : null);
            setTarget(same ? activeDownload.target : '');
            setPreview(null);
            return;
        }
        if (!busy) discardPrepared();
        draftGeneration.current++;
        setSelected(item);
        setProjectEngine('');
        setProjectName(suggestedName(item.title));
        setArtifact('');
        setPlan(null);
        setError('');
        setMessage('');
        if (!busy) setProgress(null);
        setQueueError('');
        void invoke<{vaultPath: string}>('vault_storage_get').then(options => {if (options) setQueueVaultPath(options.vaultPath);}).catch(() => {});
        setMode(item.kind === 'Plugin' || downloadOnly || busy ? 'vault' : installNow ? 'install' : null);
        setTarget('');
        setPreview(null);
        setAcceptVersion(false);
    };
    const openUpdate = async (item: FabItem, artifactId: string, engine: string, currentId: string) => {
        if (busy || updateLock.current || transferLock.current)
            return;
        updateLock.current = true;
        transferLock.current=true;
        setBusy(true);
        setError('');
        setMessage('');
        setInstalling(false);
        const report = (value: UpdateTransfer) => setUpdateTransfers(current => ({ ...current, [currentId]: value }));
        report({ state: 'preparing', phase: 'Подготавливаем обновление…', percent: null });
        try {
            const next = await invoke<VaultPackage>('vault_prepare', { id: item.id, artifact: artifactId, engineVersion: engine });
            setActiveDownload({ item: {...item, kind: next.kind}, plan: next, artifact: JSON.stringify([artifactId, engine]), projectEngine: engine, target: '', projectName: '', mode: 'vault' });
            const initial = { phase: 'Подключаемся к серверу…', done: 0, total: next.bytes, filesDone: 0, filesTotal: next.files, networkBytes: 0, bytesPerSecond: 0 };
            setProgress(initial);
            report({ state: 'downloading', phase: initial.phase, percent: 0 });
            const onProgress = new Channel<Progress>(p => { setProgress(p); report({ state: 'downloading', phase: p.paused?'На паузе':p.phase, percent: Math.min(100, Math.floor(p.done / Math.max(1, p.total) * 100)) }); });
            const downloaded = await invoke<VaultPackage>('vault_download', { id: next.id, onProgress,intent:{mode:'vault',project:'',projectName:'',engineVersion:engine,updateId:currentId} });
            setHistory(current => recordDownload(current, downloaded));
            await refresh();
            report({ state: 'done', phase: 'Обновлено в Vault', percent: 100, newId: downloaded.id });
        }
        catch (e) {
            report({ state: 'error', phase: String(e), percent: null });
        }
        finally {
            updateLock.current = false;
            transferLock.current=false;
            setBusy(false);
            setProgress(null);
            setActiveDownload(null);
            setInstalling(false);
            await refreshPending();
        }
    };
    const versionChoices = materialVersions(selected?.versions || []);
    const chosenVersion = versionChoices.find(v => v.key === artifact);
    const prepare = async () => {
        if (busy || !selected || !chosenVersion)
            return;
        const generation = draftGeneration.current;
        setBusy(true);
        setError('');
        setMessage('');
        setPlan(null);
        try {
            const retained = preparedDraft.current;
            const next = retained?.assetId === selected.id && retained.artifact === artifact ? retained.plan : await invoke<VaultPackage>('vault_prepare', { id: selected.id, artifact: chosenVersion.artifactId, engineVersion: chosenVersion.engine });
            if (generation !== draftGeneration.current) {await invoke('vault_discard').catch(() => {}); return;}
            preparedDraft.current = {assetId: selected.id, artifact, plan: next};
            if (next.kind !== selected.kind) {
                setSelected({...selected, kind: next.kind});
                setTarget('');
                setProjectName(suggestedName(next.title));
                setPreview(null);
                setAcceptVersion(false);
                if (mode === 'install') {
                    setMessage(correctedKindMessage);
                    return;
                }
            }
            setPlan(next);
            if (mode === 'install') {
                setPreview(await invoke<Preview>('vault_prepared_preview', { id: next.id, project: target, projectName: next.kind === 'Project' ? projectName : null }));
            }
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
        }
    };
    const enqueue = async () => {
        if (!selected || !chosenVersion || queueBusy) return;
        const item = selected;
        if (await queueCommand('vault_queue_enqueue', {request: {assetId: item.id, artifactId: chosenVersion.artifactId, selectedEngine: chosenVersion.engine, title: item.title}})) {
            setSelected(current => current?.id === item.id ? null : current);
        }
    };
    const download = async () => {
        if (!plan || !selected || busy)
            return;
        await runDownload({item:selected,plan,artifact,projectEngine,target,projectName,mode});
    };
    const runDownload=async(context:{item:FabItem;plan:VaultPackage;artifact:string;projectEngine:string;target:string;projectName:string;mode:'vault'|'install'|null;updateId?:string})=>{
        if(transferLock.current)return;transferLock.current=true;
        const {plan,artifact,projectEngine,target,projectName,mode,updateId}=context;
        const selected = {...context.item, kind: plan.kind};
        setActiveDownload({ item: selected, plan, artifact, projectEngine, target, projectName, mode });
        setInstalling(false);
        setBusy(true);
        setError('');
        setMessage('');
        setProgress({ phase: 'Подключаемся к серверу загрузки…', done: 0, total: plan.bytes, filesDone: 0, filesTotal: plan.files, networkBytes: 0, bytesPerSecond: 0 });
        const report=(state:'downloading'|'done'|'error',phase:string,percent:number|null,newId?:string)=>{if(updateId)setUpdateTransfers(current=>({...current,[updateId]:{state,phase,percent,newId}}));};
        const onProgress = new Channel<Progress>(p=>{setProgress(p);report('downloading',p.paused?'На паузе':p.phase,Math.min(100,Math.floor(p.done/Math.max(1,p.total)*100)));});
        try {
            const downloaded = await invoke<VaultPackage>('vault_download', { id: plan.id, onProgress,intent:{mode:mode||'vault',project:target,projectName,engineVersion:projectEngine,updateId} });
            setHistory(current => recordDownload(current, downloaded));
            await refresh();
            if (mode === 'install' && downloaded.kind !== plan.kind) {
                setSelected(null);
                openInstall(downloaded);
                setMessage(correctedKindMessage);
            }
            else if (mode === 'install') {
                setInstalling(true);
                setProgress({ phase: 'Проверяем файлы проекта', done: 0, total: 0, filesDone: 0, filesTotal: plan.files, networkBytes: 0, bytesPerSecond: 0,canPause:false });
                const n = await invoke<number>('vault_install', { id: downloaded.id, project: target, projectName: downloaded.kind === 'Project' ? projectName : null, engineVersion: downloaded.kind === 'Project' ? projectEngine : null,onProgress });
                await onProjectsChanged();
                setMessage(`${plan.title} — добавлено файлов: ${n}. ${target}`);
            }
            else {
                setMessage(`${plan.title} — загружено и проверено`);
            }
            setSelected(current => current?.id === selected.id ? null : current);
            report('done','Обновлено в Vault',100,downloaded.id);
        }
        catch (e) {
            setError(String(e));
            report('error',String(e),null);
        }
        finally {
            setBusy(false);
            setProgress(null);
            setActiveDownload(null);
            setInstalling(false);
            transferLock.current=false;await refreshPending();
        }
    };
    const resumeDownload=async(id:string)=>{
        if(busy||transferLock.current)return;transferLock.current=true;setBusy(true);setRestoringDownload(true);setError('');
        let context:Parameters<typeof runDownload>[0]|null=null;
        try{
            const prepared=await invoke<VaultPackage>('vault_resume_prepare',{id});
            const cached=await invoke<{items:FabItem[]}|null>('fab_cached_library').catch(()=>null);
            const item={...(cached?.items.find(i=>i.id===prepared.assetId)||{id:prepared.assetId||prepared.id,engineVersions:prepared.engines,categories:[],source:'Fab',versions:[]}),title:prepared.title,kind:prepared.kind};
            if (prepared.artifactId && !item.versions.some(version=>version.artifactId===prepared.artifactId && version.engines.includes(prepared.selectedEngine||''))) item.versions=[...item.versions,{artifactId:prepared.artifactId,engines:[prepared.selectedEngine||prepared.engines[0]],platforms:['Windows']}];
            const task=pendingDownloads.find(t=>t.id===id);
            const intent=task?.intent;
            context={item,plan:prepared,artifact:JSON.stringify([prepared.artifactId,prepared.selectedEngine]),projectEngine:intent?.engineVersion||prepared.selectedEngine||'',target:intent?.project||'',projectName:intent?.projectName||'',mode:intent?.mode||'vault',updateId:intent?.updateId};
            if (intent?.mode==='install' && (task?.kind!==prepared.kind || (prepared.kind==='Project' && /\.uproject$/i.test(intent.project)))) {
                draftGeneration.current++;
                preparedDraft.current={assetId:item.id,artifact:context.artifact,plan:prepared};
                setSelected(item);setInstallItem(null);setPlan(null);setPreview(null);setAcceptVersion(false);
                setProjectEngine(context.projectEngine);setArtifact(context.artifact);setTarget('');setProjectName(suggestedName(item.title));setMode(prepared.kind==='Plugin'?'vault':'install');setMessage(correctedKindMessage);
                context=null;
                await refreshPending();
                return;
            }
            setPlan(prepared);setProjectEngine(context.projectEngine);setArtifact(context.artifact);setTarget(context.target);setProjectName(context.projectName);setMode(context.mode);
        }catch(e){setError(String(e));}finally{transferLock.current=false;setBusy(false);setRestoringDownload(false);}
        if(context)await runDownload(context);
    };
    const forgetDownload=async(id:string)=>{await invoke('vault_resume_forget',{id});await refreshPending();};
    useEffect(()=>{const handler=(event:Event)=>{void resumeDownload((event as CustomEvent<string>).detail);};window.addEventListener('um-resume-vault',handler);return()=>window.removeEventListener('um-resume-vault',handler);},[busy,pendingDownloads]);
    const openInstall = (requested: VaultPackage, destination?: 'project' | 'engine') => { const current=items.find(current=>current.id===requested.id); const item=requested.kind==='Asset' && current && current.kind!=='Asset'?current:requested; setPluginChoice(item.kind === 'Plugin' && !destination); setPluginDestination(destination || 'project'); setInstallItem(item); setArtifact(''); setProjectEngine(''); setProjectName(suggestedName(item.title)); setTarget(''); setPreview(null); setAcceptVersion(false); setError(''); setMessage(''); };
    const inspect = async () => {
        if (!installItem)
            return;
        setBusy(true);
        setError('');
        setPreview(null);
        setAcceptVersion(false);
        try {
            setPreview(await invoke<Preview>('vault_install_preview', { id: installItem.id, project: target, projectName: installItem.kind === 'Project' ? projectName : null, engineVersion: installItem.kind === 'Project' ? projectEngine : null }));
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
        }
    };
    const install = async () => {
        if (!installItem || !preview)
            return;
        setBusy(true);
        setInstalling(true);
        setError('');
        setProgress({phase:'Проверяем файлы проекта',done:0,total:0,filesDone:0,filesTotal:preview.files,networkBytes:0,bytesPerSecond:0,canPause:false});
        const onProgress=new Channel<Progress>(setProgress);
        try {
            const n = await invoke<number>('vault_install', { id: installItem.id, project: target, projectName: installItem.kind === 'Project' ? projectName : null, engineVersion: installItem.kind === 'Project' ? projectEngine : null,onProgress });
            await onProjectsChanged();
            setMessage(`Установка завершена: добавлено файлов ${n}. ${preview.target}`);
            setInstallItem(null);
        }
        catch (e) {
            setError(String(e));
        }
        finally {
            setBusy(false);
            setInstalling(false);
            setProgress(null);
        }
    };
    const progressStep = !!selected && activeDownload?.item.id === selected.id;
    const queueActive = queue.items.find(item => item.id === queue.activeId);
    const queuedCount = queue.items.filter(item => item.id !== queue.activeId && !isManualQueueTransfer(item, activeDownload)).length;
    const engineInstall = installItem?.kind === 'Plugin' && pluginDestination === 'engine';
    const pluginOptions = installItem && <><label className="wizard-field">{tr("Скачанная версия")}<select aria-label={tr("Скачанная версия плагина")} value={installItem.id} disabled={busy} onChange={e => { const item = items.find(p => p.id === e.target.value); if (item) {
        setInstallItem(item);
        setTarget('');
        setPreview(null);
        setAcceptVersion(false);
    } }}>{tr(items.filter(p => p.kind === 'Plugin' && (installItem.assetId ? p.assetId === installItem.assetId : p.title === installItem.title)).map(p => <option key={p.id} value={p.id}>{tr("UE ")}{tr(p.engines.map(engineVersion).join(', ') || 'не указана')}{tr(" · ")}{tr(p.source || 'Unreal Manager')}</option>))}</select></label><p className="muted">{installItem.path}</p><div className="asset-actions"><button disabled={busy} onClick={() => { setPluginDestination('project'); setPluginChoice(false); setTarget(''); }}><FolderOpen size={20}/>{tr("Копировать в проект")}</button><button disabled={busy} onClick={() => { setPluginDestination('engine'); setPluginChoice(false); setTarget(''); }}><PackageOpen size={20}/>{tr("Установить на движок")}</button></div></>;
    const compatibleEngines = engines.filter(e => installItem?.engines.map(engineVersion).includes(e.version));
    const engineField = <label className="wizard-field">{tr("Установленный движок")}<select aria-label={tr("Движок для плагина")} disabled={busy} value={target} onChange={e => { setTarget(e.target.value); setPreview(null); }}><option value="">{tr("Выберите Unreal Engine")}</option>{tr(compatibleEngines.map(e => <option key={e.path} value={e.path}>{tr("UE ")}{e.version}{tr(" · ")}{e.path}</option>))}</select>{tr(!compatibleEngines.length && <small className="muted">{tr("Совместимый движок не установлен. Скачайте другую версию плагина или установите подходящий Unreal Engine.")}</small>)}</label>;
    const nameField = <label className="wizard-field">{tr("Имя файла проекта")}<div className="project-name-field"><input aria-label={tr("Имя файла проекта")} value={projectName} disabled={busy} maxLength={64} onChange={e => { setProjectName(e.target.value); setPlan(null); setPreview(null); setAcceptVersion(false); }}/><span>{tr(".uproject")}</span></div><small className="muted">{tr("Латинские буквы, цифры и подчёркивание. Начните с буквы.")}</small></label>;
    const supportedEngines = (installItem?.engines || selected?.engineVersions || []).map(engineVersion);
    const validProjectEngine = /^\d+\.\d+(?:\.\d+)?$/.test(projectEngine);
    const projectEngineField = <label className="wizard-field">{tr("Движок для нового проекта")}<input aria-label={tr("Движок для нового проекта")} list="project-engine-options" placeholder={tr("Например, 5.8")} value={projectEngine} disabled={busy} onChange={e => { setProjectEngine(e.target.value); setPlan(null); setPreview(null); }}/><datalist id="project-engine-options">{tr([...new Set([...engines.map(e => e.version), ...supportedEngines])].sort((a, b) => b.localeCompare(a, undefined, { numeric: true })).map(v => <option key={v} value={v}/>))}</datalist><small className="muted">{tr("Можно указать любую версию UE.")}{tr(validProjectEngine && !supportedEngines.includes(projectEngine) ? ' Автор не заявил поддержку этой версии; совместимость не гарантирована.' : '')}</small></label>;
    const status = <>{tr(progress && <div className="library-loading" role="status"><Download size={24}/><div className="download-progress"><strong>{tr(progress.paused?'На паузе':progress.phase)}</strong><progress max={Math.max(1, progress.total)} value={progress.total ? progress.done : undefined}/><p>{tr("Обработано")}: {tr(size(progress.done))}{progress.total > 0 && <>{tr(" из ")}{tr(size(progress.total))}</>}{tr(" · Файлы ")}{tr(progress.filesDone)}{tr(" / ")}{tr(progress.filesTotal)}</p><small>{tr("По сети: ")}{tr(size(progress.networkBytes))}{tr(" · ")}{tr(size(progress.bytesPerSecond))}{tr("/с")}</small><div className="vault-operation-actions">{!installing&&<VaultPauseButton paused={progress.paused} canPause={progress.canPause}/>}<button disabled={installing} onClick={() => invoke('vault_cancel').catch(e => setError(String(e)))}>{tr("Остановить загрузку")}</button></div></div></div>)}<EpicAuthError error={error}/>{tr(message && <p role="status" className="vault-success">{tr(message)}</p>)}</>;
    const epicSettings = <section className="epic-vault-settings"><h3>{tr("Vault от Epic Games Launcher")}</h3>
    <p className="muted">{tr("Подключите папку VaultCache. Скачанные материалы остаются в ней и доступны для добавления в проект.")}</p>
    <label htmlFor="epic-vault-path">{tr("Папка VaultCache")}</label><div className="epic-vault-controls"><input id="epic-vault-path" aria-label={tr("Папка Epic VaultCache")} placeholder={tr("Папка Epic VaultCache")} value={epicDirectory} disabled={busy} onChange={e => setEpicDirectory(e.target.value)}/><button disabled={busy} onClick={async()=>{try{const path=await invoke<string|null>('choose_project_location',{folder:true});if(path)setEpicDirectory(path);}catch(e){setError(String(e));}}}><FolderOpen size={17}/>{tr('Выбрать папку')}</button><button onClick={syncEpic} disabled={busy || !epicDirectory.trim()}><RefreshCw size={17} className={epicScan ? 'spin' : ''}/>{tr(epicScan ? 'Проверяем…' : 'Синхронизировать')}</button></div>
    {tr(epicScan && <p role="status">{tr("Проверяем папки: ")}{tr(epicScan.done)}{tr(" / ")}{tr(epicScan.total || '…')}</p>)}
    {tr(epicResult?.directory && <><p role="status">{tr("Подключено материалов: ")}{tr(epicResult.items.length)}{tr(". Папка: ")}{tr(epicResult.directory)}</p><p className="muted">{tr("При синхронизации проверяются наличие и размеры файлов. Перед копированием в проект проверяется их целостность.")}</p>{tr(epicResult.warnings.length > 0 && <details><summary>{tr("Не подключено: ")}{tr(epicResult.warnings.length)}</summary>{tr(epicResult.warnings.map((warning, i) => <p key={i}>{tr(warning)}</p>))}</details>)}</>)}
    <EpicAuthError error={error}/>
  </section>;
    return {queue, queueActive, queuedCount, pendingProgress:Object.fromEntries(pendingDownloads.map(t=>[t.assetId,{id:t.assetId,taskId:t.id,percent:Math.min(100,Math.floor(t.done/Math.max(1,t.total)*100)),phase:t.phase==='На паузе'?'На паузе':'Ожидает продолжения'}])), select, openUpdate, openProjectUpdate, updateTransfers, busy, items, epicSettings, refresh, openInstall, notice: status,
        tileProgress: activeDownload && progress ? { id: activeDownload.item.id, percent: Math.min(100, Math.max(0, Math.floor(progress.done / Math.max(1, progress.total) * 100))), phase: progress.paused?'На паузе':progress.phase } : queueActive?.progress ? {id:queueActive.assetId,percent:Math.min(100,Math.floor(queueActive.progress.done/Math.max(1,queueActive.progress.total)*100)),phase:queueActive.progress.paused?'На паузе':queueActive.progress.phase} : null,
        view: <DownloadsView queue={queue} queueBusy={queueBusy} queueError={queueError} onQueuePaused={paused=>void queueCommand('vault_queue_set_paused',{paused})} onQueueReorder={(ids,expectedRevision)=>void queueCommand('vault_queue_reorder',{ids,expectedRevision})} onQueueRemove={(id,expectedRevision)=>void queueCommand('vault_queue_remove',{id,expectedRevision})} onQueueRetry={(id,expectedRevision)=>void queueCommand('vault_queue_retry',{id,expectedRevision})} restoring={restoringDownload} pending={pendingDownloads} onResume={id=>void resumeDownload(id)} onForget={forgetDownload} engineDownload={engineDownload} active={activeDownload} progress={progress} installing={installing} records={history.records} error={error} historyError={historyError} onClear={id => setHistory(current => clearDownloadHistory(current, id))} onStop={() => void invoke('vault_cancel').catch(e => setError(String(e)))} onDetails={() => { if (activeDownload)
            select(activeDownload.item); }} onUse={item => item.kind === 'Plugin' ? onPlugins() : openInstall(item)} busy={busy} available={items}/>,
        dialog: <>{tr(updateRequest && <ProjectUpdateDialog items={items} projects={projects} engines={engines} initial={updateRequest.item} initialNewId={updateRequest.newId} busy={busy} onBusy={setBusy} onClose={() => setUpdateRequest(null)} onDone={async () => { await refresh(); await onProjectsChanged(); }}/>)}{tr(selected && <ModalFocusScope onClose={closeSelected}><section className="login-dialog vault-dialog" role="dialog" aria-modal="true" aria-label={selected.title}><div className="section-head"><h2>{selected.title}</h2><button onClick={closeSelected} aria-label={tr("Закрыть загрузку")}><X /></button></div>
      <div className="asset-details"><FabCover item={selected}/><div><p>{tr({ Asset: 'Ассет', Plugin: 'Плагин', Project: 'Проект' }[selected.kind] || selected.kind)}{tr(" · ")}{tr(selected.source)}</p><p>{tr(selected.categories.join(' · '))}</p><p>{tr("Версии UE: ")}{tr(selected.engineVersions.join(', ') || 'Не указаны')}</p></div></div>
      {tr(!mode && selected.description && <p className="asset-description">{tr(selected.description.replace(/<[^>]*>/g, ' ').replace(/&nbsp;/g, ' '))}</p>)}
      {mode === 'vault' && !progressStep ? <div className="vault-queue-draft">
        <ol className="vault-steps" aria-label={tr('Этапы')}><li className="done">{tr('1. Действие')}</li><li className="current">{tr('2. Параметры')}</li></ol>
        <label className="wizard-field">{tr('Версия Unreal Engine')}<select aria-label={tr('Версия Unreal Engine для материала')} disabled={queueBusy} value={artifact} onChange={e => {setArtifact(e.target.value);setProjectEngine(versionChoices.find(v=>v.key===e.target.value)?.engine||'');setQueueError('');}}><option value="">{tr('Выберите версию UE')}</option>{versionChoices.map(version=><option key={version.key} value={version.key}>UE {version.engine}{versionChoices.filter(other=>other.engine===version.engine).length>1?` · ${version.artifactId}`:''}</option>)}</select></label>
        {!versionChoices.length && <p role="alert">{tr('Нет вариантов с указанной версией UE для Windows.')}</p>}
        {queueVaultPath && <p className="project-delete-path">{tr('Папка хранилища')}: {queueVaultPath}</p>}
        <p className="muted">{tr('Сведения о файлах и свободное место будут проверены перед запуском.')}</p>
        <p className="muted">{tr('Очередь выполняется только пока открыт Unreal Manager.')}</p>
        {queueError && <p className="error-text" role="alert">{tr(queueError)}</p>}
        <div className="wizard-footer"><button disabled={queueBusy} onClick={()=>setMode(null)}>{tr('Назад')}</button><button className="primary" disabled={queueBusy||!chosenVersion} onClick={()=>void enqueue()}><Download size={17}/>{tr('Добавить в очередь')}</button></div>
      </div> : <>
      <ol className="vault-steps" aria-label={tr("Этапы")}><li className={!mode && !progressStep ? "current" : "done"}>{tr("1. Действие")}</li><li className={progressStep ? "done" : mode && !plan ? "current" : plan ? "done" : ""}>{tr("2. Параметры")}</li><li className={progressStep ? "done" : plan ? "current" : ""}>{tr("3. Проверка")}</li><li className={progressStep ? "current" : ""}>{tr("4. Загрузка")}</li></ol>{tr(!progressStep && <>{tr(!mode && <div className="asset-actions"><button disabled={queueBusy} className={mode === 'vault' ? 'primary' : ''} onClick={() => { setMode('vault'); discardPrepared(); }}>{tr("Скачать в хранилище")}</button>{tr(selected.kind !== 'Plugin' && <button disabled={busy} className={mode === 'install' ? 'primary' : ''} onClick={() => { setMode('install'); setPlan(null); setPreview(null); }}>{tr(materialAction(selected.kind))}</button>)}</div>)}
      {tr(mode && !plan && <label className="wizard-field">{tr("Версия Unreal Engine")}<select aria-label={tr("Версия Unreal Engine для материала")} disabled={busy} value={artifact} onChange={e => { setArtifact(e.target.value); setProjectEngine(versionChoices.find(v => v.key === e.target.value)?.engine || ''); setPlan(null); setPreview(null); setAcceptVersion(false); }}><option value="">{tr("Выберите версию UE")}</option>{tr(versionChoices.map(v => <option key={v.key} value={v.key}>{tr("UE ")}{tr(v.engine)}{tr(versionChoices.filter(other => other.engine === v.engine).length > 1 ? ` · ${v.artifactId}` : '')}</option>))}</select>{tr(chosenVersion?.shared && <small className="muted">{tr("Автор использует общий пакет для нескольких версий UE.")}{tr(selected.kind === 'Project' && mode === 'install' ? ` Новый проект будет открываться в UE ${projectEngine}.` : '')}</small>)}{tr(!versionChoices.length && <small role="alert">{tr("Нет вариантов с указанной версией UE для Windows.")}</small>)}</label>)}
      {tr(mode === 'install' && !plan && <div className="wizard-fields">{tr(selected.kind === 'Project' && <>{tr(nameField)}{tr(projectEngineField)}</>)}<p>{tr(selected.kind === 'Project' ? 'Куда создать новый проект?' : 'В какой проект добавить материал?')}</p>{tr(selected.kind !== 'Project' && <select aria-label={tr("Проект для материала")} disabled={busy} value={target} onChange={e => { setTarget(e.target.value); setPlan(null); setPreview(null); setAcceptVersion(false); }}><option value="">{tr("Выберите проект")}</option>{tr(projects.map(p => <option key={p.path} value={p.path}>{p.name}{tr(" · ")}{tr(p.engineAssociation || 'версия не указана')}</option>))}</select>)}<div className="path-picker"><input aria-label={tr("Назначение материала")} disabled={busy} value={target} onChange={e => { setTarget(e.target.value); setPlan(null); setPreview(null); setAcceptVersion(false); }} placeholder={tr(selected.kind === 'Project' ? 'Полный путь к новому каталогу' : 'Или полный путь к .uproject')}/><button disabled={busy || picking} onClick={() => browse(selected.kind, selected.title)}><FolderOpen size={17}/>{tr(picking ? 'Открываем…' : selected.kind === 'Project' ? 'Выбрать папку…' : 'Выбрать проект…')}</button></div><p className="muted">{tr(selected.kind === 'Project' ? 'Проект разворачивается из Vault в новую папку. Если эта версия ещё не скачана, сначала она будет сохранена в Vault.' : 'Сначала материал сохраняется в Vault, затем автоматически добавляется в выбранный проект.')}</p></div>)}
      {tr(!selected.versions.length && <p>{tr("Fab не предоставил варианты загрузки этого материала.")}</p>)}
      {tr(mode && !plan && <div className="wizard-footer"><button disabled={busy || picking} onClick={() => { setMode(null); setPreview(null); }}>{tr("Назад")}</button><button className="primary" disabled={busy || picking || !chosenVersion || (mode === 'install' && (!target || (selected.kind === 'Project' && (!projectName || !validProjectEngine))))} onClick={prepare}>{tr(busy ? 'Получаем сведения о файлах…' : 'Далее')}</button></div>)}
      {tr(plan && <><div className="wizard-summary"><p>{tr("Выбрано: ")}<b>{tr("UE ")}{tr(chosenVersion?.engine)}</b>{tr(mode === 'install' && selected.kind === 'Project' ? ` · Новый проект: UE ${projectEngine}` : '')}</p><p>{tr("На диске: ")}<b>{tr(size(plan.bytes))}</b>{tr(" · ")}{tr(plan.files)}{tr(" файлов")}</p><p className="muted">{tr(items.some(p => p.id === plan.id) ? 'Эта версия уже в Vault. Повторно скачиваться не будет.' : `${tr("Размер загрузки")}: ${size(plan.downloadBytes)} · ${plan.path}`)}</p>
      </div>{tr(mode === 'install' && preview && <><p>{tr("Файл проекта: ")}{tr(preview.projectFile || preview.target)}{tr(" · Файлов: ")}{tr(preview.files)}</p>{tr(preview.conflicts.length > 0 ? <p className="error-text">{tr("Конфликтов: ")}{tr(preview.conflicts.length)}{tr(". Файлы не будут перезаписаны.")}</p> : <p>{tr("Конфликтов нет. Уже совпадают: ")}{tr(preview.existing)}</p>)}{tr(!preview.compatible && <label className="remember"><input type="checkbox" checked={acceptVersion} onChange={e => setAcceptVersion(e.target.checked)}/>{tr("Версия проекта отличается от заявленной. Использовать выбранный вариант материала.")}</label>)}</>)}
      <div className="wizard-footer"><button disabled={busy || picking} onClick={discardPrepared}>{tr("Назад")}</button><button className="primary" disabled={busy || (mode === 'install' && (!preview || preview.conflicts.length > 0 || (!preview.compatible && !acceptVersion)))} onClick={download}><Download size={17}/>{tr(mode === 'install' ? (selected.kind === 'Project' ? (items.some(p => p.id === plan.id) ? 'Создать проект' : 'Скачать и создать проект') : (items.some(p => p.id === plan.id) ? materialAction(selected.kind) : selected.kind === 'Plugin' ? 'Скачать и установить плагин' : 'Скачать и добавить в проект')) : 'Скачать в хранилище')}</button></div></>)}
      </>)}{tr(progressStep ? <div className="wizard-transfer"><h3>{tr(installing ? materialAction(selected.kind) : 'Скачивание в хранилище')}</h3>{tr(status)}<p className="muted">{tr("Можно закрыть это окно. Загрузка продолжится, прогресс останется на плитке и в разделе «Загрузки».")}</p><div className="wizard-footer"><button onClick={() => setSelected(null)}>{tr("Закрыть окно")}</button></div></div> : !activeDownload && status)}
      </>}
    </section></ModalFocusScope>)}
    {tr(installItem && <ModalFocusScope onClose={()=>setInstallItem(null)} closeDisabled={installing}><section className="login-dialog vault-dialog" role="dialog" aria-modal="true" aria-label={installItem.title}><div className="section-head"><h2>{installItem.title}</h2><button disabled={installing} onClick={() => setInstallItem(null)} aria-label={tr("Закрыть установку")}><X /></button></div>
      {tr(installItem.kind === 'Plugin' ? <ol className="vault-steps" aria-label={tr("Этапы установки плагина")}><li className={pluginChoice ? 'current' : 'done'}>{tr("1. Версия и действие")}</li><li className={pluginChoice ? '' : preview ? 'done' : 'current'}>{tr("2. Место установки")}</li><li className={preview ? 'current' : ''}>{tr("3. Проверка")}</li></ol> : <ol className="vault-steps" aria-label={tr("Этапы")}><li className={!preview ? 'current' : 'done'}>{tr("1. Место установки")}</li><li className={preview ? 'current' : ''}>{tr("2. Проверка")}</li></ol>)}{tr(pluginChoice && pluginOptions)}{tr(!preview && !pluginChoice && <><p>{tr(installItem.kind === 'Project' ? 'Укажите полный путь к новому каталогу проекта.' : engineInstall ? 'Выберите установленный совместимый движок.' : 'Выберите проект. Существующие отличающиеся файлы не перезаписываются.')}</p>
      {tr(installItem.kind === 'Project' && <>{tr(nameField)}{tr(projectEngineField)}</>)}{tr(engineInstall && engineField)}{tr(!engineInstall && installItem.kind !== 'Project' && <select disabled={busy} value={target} onChange={e => { setTarget(e.target.value); setPreview(null); }}><option value="">{tr("Выберите проект")}</option>{tr(projects.map(p => <option key={p.path} value={p.path}>{p.name}{tr(" · ")}{tr(p.engineAssociation || 'версия не указана')}</option>))}</select>)}
      {tr(!engineInstall && <div className="path-picker"><input aria-label={tr("Путь установки")} disabled={busy} value={target} onChange={e => { setTarget(e.target.value); setPreview(null); }} placeholder={tr(installItem.kind === 'Project' ? 'Путь установки' : 'Полный путь к .uproject')}/>
      <button disabled={busy || picking} onClick={() => browse(installItem.kind, installItem.title)}><FolderOpen size={17}/>{tr(picking ? 'Открываем…' : installItem.kind === 'Project' ? 'Выбрать папку…' : 'Выбрать проект…')}</button></div>)}<div className="wizard-footer"><button hidden={installItem.kind !== 'Plugin'} disabled={busy || picking} onClick={() => { setPluginChoice(true); setTarget(''); setPreview(null); setAcceptVersion(false); }}>{tr("Назад")}</button><button className="primary" disabled={busy || picking || !target || (installItem.kind === 'Project' && (!projectName || !validProjectEngine))} onClick={inspect}>{tr(busy ? 'Проверяем файлы…' : 'Далее')}</button></div></>)}
      {tr(preview && <><button disabled={busy} onClick={() => { setPreview(null); setAcceptVersion(false); }}>{tr("Назад")}</button><p>{tr(preview.files)}{tr(" файлов · Уже совпадают: ")}{tr(preview.existing)}</p><p className="muted">{tr(preview.projectFile || preview.target)}</p>{tr(preview.conflicts.length > 0 ? <p role="alert" className="error-text">{tr("Конфликтов: ")}{tr(preview.conflicts.length)}{tr(". Установка заблокирована.")}<br />{tr(preview.conflicts.slice(0, 5).join('\n'))}</p> : <><p>{tr("Конфликтов файлов нет.")}</p>{tr(!preview.compatible && <label className="remember"><input type="checkbox" checked={acceptVersion} onChange={e => setAcceptVersion(e.target.checked)}/>{tr("Версия проекта не совпадает с заявленными версиями материала (")}{tr(installItem.engines.join(', '))}{tr("). Установить выбранную версию.")}</label>)}<button className="primary" disabled={busy || (!preview.compatible && !acceptVersion)} onClick={install}>{tr(busy ? 'Устанавливаем…' : materialAction(installItem.kind))}</button></>)}</>)}{tr(status)}
    </section></ModalFocusScope>)}</>
    };
}
