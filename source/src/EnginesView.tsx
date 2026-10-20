import {type EngineTransfer, type EngineProgress, transferSize, formatEnginePhase, enginePercentText, engineProgressPercent, engineProgressMetrics} from './EngineTransferCard';
import {EngineResumeTasks} from './EngineResumeTasks';
import {EnginePauseButton} from './EnginePauseButton';
import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Channel, invoke } from '@tauri-apps/api/core';
import { Download, FolderOpen, RefreshCw, Rocket, X, Trash2, SlidersHorizontal, Square, ArrowLeft } from 'lucide-react';
import { EngineDelete } from './EngineDelete';
import { EngineTile } from './EngineTile';
import { PageActions } from './PageActions';
import type { EngineInstall } from './lib/backend';
type Installed = {
    path: string;
    build: string;
    version: string;
    updateAvailable: boolean;
};
type Release = {
    appName: string;
    version: string;
    build: string;
    installed: Installed[];
};
type Component = {
    tag: string;
    label: string;
    bytes: number;
    selected: boolean;
};
type Preview = {
    id: string;
    appName: string;
    build: string;
    path: string;
    update: boolean;
    components: Component[];
    bytes: number;
    downloadBytes: number;
    changed: number;
    added: number;
    removed: number;
    requiredBytes: number;
    freeBytes: number;
    conflicts: string[];
};
type Progress = EngineProgress & {networkBytes:number;bytesPerSecond?:number};
const size = (n: number) => `${(n / 1073741824).toLocaleString(getLocale(), { maximumFractionDigits: 1 })} ГБ`;
const compareVersions = (a: string, b: string) => b.localeCompare(a, 'en', { numeric: true });
export function EnginesView({ engines, onRefresh, onProgress, active = true }: {
    active?: boolean;
    engines: EngineInstall[];
    onRefresh: () => Promise<void>;
    onProgress: (progress: EngineTransfer | null) => void;
}) {
    const [deletion, setDeletion] = useState<string | null>(null);
    const [pending, setPending] = useState<{
        path: string;
        build: string;
    }[]>([]);
    const [releases, setReleases] = useState<Release[]>([]), [checking, setChecking] = useState(false), [error, setError] = useState(''), [tab, setTab] = useState<'installed' | 'available'>('installed');
    const [choice, setChoice] = useState<Release | null>(null), [path, setPath] = useState(''), [preview, setPreview] = useState<Preview | null>(null), [busy, setBusy] = useState(false), [progress, setProgress] = useState<Progress | null>(null), [result, setResult] = useState(''), [open, setOpen] = useState(false), [activeApp, setActiveApp] = useState('');
    const lock = useRef(false), visible = useRef(false);
    const selectedComponents = useRef<string[] | undefined>(undefined);
    const [isUpdate, setIsUpdate] = useState(false);
    const [isModify, setIsModify] = useState(false);
    const [resuming,setResuming]=useState(false);
    const check = async () => { setChecking(true); setError(''); try {
        setPending(await invoke<{
            path: string;
            build: string;
        }[]>('engine_pending'));
        setReleases(await invoke<Release[]>('engine_catalog', { cachedOnly: false }));
    }
    catch (e) {
        setError(String(e));
    }
    finally {
        setChecking(false);
    } };
    useEffect(() => { let cancelled = false; void (async () => { try {
        const cached = await invoke<Release[]>('engine_catalog', { cachedOnly: true });
        if (!cancelled && cached.length)
            setReleases(cached);
    }
    catch { } if (!cancelled)
        await check(); })(); return () => { cancelled = true; }; }, []);
    const close = () => { visible.current = false; setOpen(false); if (!lock.current)
        void invoke('engine_discard'); };
    const select = (release: Release, destination?: string, modify = false) => {
        if (lock.current)
            return;
        setResuming(false);
        selectedComponents.current = undefined;
        setChoice(release);
        setPath(destination || '');
        setIsUpdate(!!destination);
        setIsModify(modify);
        setPreview(null);
        setProgress(null);
        setResult('');
        setError('');
        visible.current = true;
        setOpen(true);
        if (destination)
            void prepare(undefined, release, destination, true, modify);
    };
    const prepare = async (selected?: string[], release = choice, destination = path, update = isUpdate, modify = isModify) => {
        if (!release || lock.current || !destination.trim())
            return;
        lock.current = true;
        setBusy(true);
        setError('');
        setProgress({ phase: 'Определяем размер загрузки', done: 0, total: 0, percent: 0, networkBytes: 0 });
        try {
            const prepared = await invoke<Preview>('engine_prepare', { appName: release.appName, path: destination, selected: selected ?? selectedComponents.current ?? null, update, modify });
            if (visible.current) {
                selectedComponents.current = prepared.components.filter(c => c.selected).map(c => c.tag);
                setPreview(prepared);
            } else
                await invoke('engine_discard');
            setProgress(null);
        }
        catch (e) {
            setPreview(null);
            setError(String(e));
            setProgress(null);
        }
        finally {
            setBusy(false);
            lock.current = false;
        }
    };
    const install = async () => { if (!preview || lock.current)
        return; lock.current = true; setBusy(true); setError(''); setProgress({phase:'Сохраняем задачу установки',done:0,total:0,percent:0,networkBytes:0}); setActiveApp(preview.appName); let sampleTime=performance.now(), sampleBytes=0, sampleProcessed=0, samplePhase='', processingSpeed=0, speed=0; const channel = new Channel<Progress>(); channel.onmessage = p => {
        const now=performance.now();
        if(p.phase!==samplePhase){sampleTime=now;sampleBytes=p.networkBytes;sampleProcessed=p.processedBytes||0;samplePhase=p.phase;speed=0;processingSpeed=0;}
        if(now-sampleTime>=500){speed=Math.max(0,p.networkBytes-sampleBytes)*1000/(now-sampleTime);processingSpeed=Math.max(0,(p.processedBytes||0)-sampleProcessed)*1000/(now-sampleTime);sampleTime=now;sampleBytes=p.networkBytes;sampleProcessed=p.processedBytes||0;}
        if(p.phase!=='Скачиваем движок'||p.paused)speed=0;if(p.paused)processingSpeed=0;
        const current={...p,bytesPerSecond:speed,processingBytesPerSecond:processingSpeed};setProgress(current);
        onProgress({...current,title:`Unreal Engine ${preview.build.split('-')[0]}`});
    }; try {
        setResult(await invoke<string>('engine_install', { id: preview.id, onProgress: channel }));
        await onRefresh();
        await check();
    }
    catch (e) {
        setError(String(e));
    }
    finally {
        setBusy(false);
        lock.current = false;
        onProgress(null);
    } };
    const resume=async(id:string,legacyPath?:string)=>{if(lock.current)return;lock.current=true;setBusy(true);setError('');try{
        const info=await invoke<Preview>(legacyPath?'engine_resume_legacy_prepare':'engine_resume_prepare',legacyPath?{path:legacyPath}:{id});
        selectedComponents.current=undefined;setChoice({appName:info.appName,build:info.build,version:info.appName.replace('UE_',''),installed:[]});setPath(info.path);setIsUpdate(info.update);setIsModify(false);setResuming(true);setPreview(info);setProgress(null);setResult('');visible.current=true;setOpen(true);
    }catch(e){setError(String(e));}finally{setBusy(false);lock.current=false;}};
    useEffect(()=>{const handler=(event:Event)=>{void resume((event as CustomEvent<string>).detail);};window.addEventListener('um-resume-engine',handler);return()=>window.removeEventListener('um-resume-engine',handler);},[]);
    const browse = async () => { try {
        const folder = await invoke<string | null>('choose_project_location', { folder: true });
        if (folder && choice) {
            setPath(`${folder.replace(/[\\/]$/, '')}\\UE_${choice.version}`);
            setPreview(null);
        }
    }
    catch (e) {
        setError(String(e));
    } };
    const launch = async (p: string) => { try {
        await invoke('engine_launch', { path: p });
    }
    catch (e) {
        setError(String(e));
    } };
    const available = [...releases].sort((a, b) => compareVersions(a.build, b.build)).filter((r, i, all) => all.findIndex(x => x.appName === r.appName) === i);
    const installationStep = !!progress && progress.phase !== 'Определяем размер загрузки';
    const componentStep = !!preview && !result && !installationStep && (!isUpdate || isModify);
    const canReturnToLocation = !!preview && !busy && !resuming && !isUpdate && !installationStep && !result;
    const backToLocation = () => {
        if (!canReturnToLocation || lock.current || !preview) return;
        selectedComponents.current = preview.components.filter(c => c.selected).map(c => c.tag);
        setPreview(null);
        setProgress(null);
        setError('');
    };
    const local = engines.map(e => ({ e, release: releases.find(r => r.installed.some(i => i.path.toLowerCase() === e.path.toLowerCase())) }));
    return <div className="content engine-content"><div className="toolbar"><div className="tabs"><button className={tab === 'installed' ? 'selected' : ''} onClick={() => setTab('installed')}>{tr("Установленные")}</button><button className={tab === 'available' ? 'selected' : ''} onClick={() => setTab('available')}>{tr("Доступные")}</button></div><PageActions><button disabled={checking || busy} onClick={() => void check()}><RefreshCw size={17} className={checking ? 'spin' : ''}/>{tr(checking ? 'Проверяем…' : 'Проверить обновления')}</button><button className="primary" onClick={() => setTab('available')}><Download size={17}/>{tr("Установить движок")}</button></PageActions></div>
 {!busy&&<EngineResumeTasks busy={busy} legacy={pending} onResume={(id,path)=>void resume(id,path)} refreshKey={result} onRecover={async path=>{setBusy(true);try{await invoke('engine_recover',{path});await onRefresh();await check();}catch(e){setError(String(e));}finally{setBusy(false);}}}/>}
 {tr(error && !open && <p className="error-text" role="alert">{tr(error)}</p>)}{tr(busy && !open && <button className="engine-operation" onClick={() => { visible.current = true; setOpen(true); }}><RefreshCw className={progress?.paused?"":"spin"} size={17}/>{tr(progress?.paused?"На паузе":formatEnginePhase(progress?.phase || 'Операция с движком'))}{progress && engineProgressPercent(progress)!==undefined ? ` · ${enginePercentText(progress)}` : ''}</button>)}
 {tr(tab === 'installed' ? <div className="grid" onContextMenu={event => event.preventDefault()}>{tr(local.map(({ e, release }) => { const installed = release?.installed.find(i => i.path.toLowerCase() === e.path.toLowerCase()); return <div className="card engine" key={e.path} tabIndex={0} aria-label={tr(`Unreal Engine ${installed?.version || e.version}`)}><div className="ue-logo">{tr("UE")}</div><div><span className="eyebrow">{tr("UNREAL ENGINE")}</span><div className="engine-version-heading"><h3>{tr(installed?.version || e.version)}</h3>{tr(installed?.updateAvailable && <span className="engine-update-badge">{tr("Доступна ")}{tr(release?.build.split('-')[0])}</span>)}</div><p>{e.path}</p><small>{tr("Установлена")}</small></div><div className="card-actions"><button disabled={busy} className="primary" onClick={() => void launch(e.path)}><Rocket size={16}/>{tr("Запустить")}</button><button className="engine-icon-action" title={tr("Изменить")} aria-label={tr("Изменить")} disabled={busy} onClick={() => { if (release) select(release, e.path, true); else setError(tr("Войдите в Epic и проверьте список доступных движков.")); }}><SlidersHorizontal size={19}/></button><button disabled={busy} className="danger-text engine-icon-action" title={tr("Удалить навсегда")} aria-label={tr("Удалить навсегда")} onClick={() => setDeletion(e.path)}><Trash2 size={19}/></button>{tr(release && installed?.updateAvailable && <button disabled={busy} onClick={() => select(release, e.path)}><Download size={17}/>{tr("Обновить")}</button>)}</div>{tr(activeApp === release?.appName && progress && busy && <div className="engine-tile-progress"><progress aria-label={tr("Прогресс загрузки")} aria-valuetext={enginePercentText(progress)} max={100} value={engineProgressPercent(progress)}/><span>{tr(progress.paused?"На паузе":formatEnginePhase(progress.phase))}{engineProgressPercent(progress)!==undefined ? ` · ${enginePercentText(progress)}` : ''}</span></div>)}</div>; }))}</div> : <div className="fab-grid engine-available-grid">{tr(available.map(r => <EngineTile key={r.appName} version={r.build.split('-')[0]} installed={r.installed.length > 0} busy={busy} active={active && tab === 'available'} onInstall={() => select(r)}/>))}</div>)}
 {tr(deletion && <EngineDelete path={deletion} onClose={() => setDeletion(null)} onDeleted={async () => { await onRefresh(); setPending(await invoke<{
        path: string;
        build: string;
    }[]>('engine_pending')); setReleases(await invoke<Release[]>('engine_catalog', { cachedOnly: true })); }}/>)}
 {tr(tab === 'available' && !releases.length && !checking && <p className="empty">{tr("Войдите в Epic и проверьте список доступных движков.")}</p>)}
 {tr(open && choice && active && createPortal(<div className="modal-backdrop engine-modal-backdrop"><section className={`modal engine-wizard${componentStep ? ' engine-component-step' : installationStep && !result ? ' engine-progress-step' : ''}`} role="dialog" aria-modal="false" aria-label={tr(isModify ? 'Компоненты Unreal Engine' : isUpdate ? 'Обновление Unreal Engine' : 'Установка Unreal Engine')}>
  <div className="modal-head"><div><span className="eyebrow">{tr("UNREAL ENGINE")}</span><h2>{tr(isModify ? 'Изменить компоненты' : isUpdate ? 'Обновление' : 'Установка')} {tr(isModify ? (choice.installed.find(i => i.path.toLowerCase() === path.toLowerCase())?.version || choice.version) : choice.build.split('-')[0])}</h2></div><button className="icon" onClick={close} aria-label={tr("Закрыть")}><X size={20}/></button></div>
  {tr(!isUpdate && <div className="wizard-steps">{preview && !resuming && !installationStep && !result ? <button type="button" className="wizard-step-back" disabled={!canReturnToLocation} onClick={backToLocation}>{tr("1. Расположение")}</button> : <span className={!preview ? 'active' : ''}>{tr("1. Расположение")}</span>}<span className={preview && !installationStep ? 'active' : ''}>{tr("2. Компоненты")}</span><span className={installationStep ? 'active' : ''}>{tr("3. Установка")}</span></div>)}
  {tr(!isUpdate && !preview && !result && <><label>{tr("Папка движка")}</label><div className="engine-path"><input value={path} placeholder={tr("Папка движка")} disabled={busy} onChange={e => setPath(e.target.value)}/><button disabled={busy} onClick={() => void browse()}><FolderOpen size={17}/>{tr("Выбрать")}</button></div><p className="muted">{tr("Новая номерная версия устанавливается в отдельную папку.")}</p><div className="wizard-footer"><button disabled={busy || !path.trim()} className="primary" onClick={() => void prepare()}>{tr("Далее")}</button></div></>)}
  {tr(isUpdate && !preview && !busy && !result && error && <button onClick={() => void prepare()}>{tr("Повторить проверку")}</button>)}
  {tr(preview && (!isUpdate || isModify) && <p className="project-delete-path">{preview.path}</p>)}
  {tr(preview && !result && <>
   {tr(!installationStep && (isUpdate && !isModify ? <div className="engine-update-size"><span>{tr("Размер загрузки")}</span><strong>{tr(size(preview.downloadBytes))}</strong><small>{tr("Установка обновится автоматически в своей папке.")}</small></div> : <><div className="engine-components"><label className="downloaded-filter"><input type="checkbox" checked disabled/>{tr("Основные файлы движка")}</label>{tr(preview.components.map(c => <label className="downloaded-filter" key={c.tag}><input type="checkbox" checked={c.selected} disabled={busy||resuming} onChange={e => void prepare(preview.components.filter(x => x.tag === c.tag ? e.target.checked : x.selected).map(x => x.tag))}/>{tr(c.label)}<small>{tr(size(c.bytes))}</small></label>))}</div><div className="update-summary">{isModify && <span>{tr(`Файлов к удалению: ${preview.removed}`)}</span>}<span>{tr("Скачать: до ")}{tr(size(preview.downloadBytes))}</span><span>{tr("Свободно: ")}{tr(size(preview.freeBytes))}</span></div></>))}
   {tr(preview.conflicts.length > 0 && <p className="error-text" role="alert">{tr("Обнаружены локальные изменения. Обновление остановлено.")}</p>)}
   {tr(!resuming && preview.requiredBytes > preview.freeBytes && <p role="alert" className="error-text">{tr("Недостаточно места. Требуется до ")}{tr(size(preview.requiredBytes))}{tr(".")}</p>)}
   {tr(!busy && <div className="wizard-footer">{tr(canReturnToLocation && <button onClick={backToLocation}><ArrowLeft size={17}/>{tr("Назад")}</button>)}<button className="primary" disabled={!!preview.conflicts.length || !resuming && preview.requiredBytes > preview.freeBytes} onClick={() => void install()}><Download size={17}/>{tr(resuming?'Продолжить установку':isModify ? 'Применить' : isUpdate ? 'Обновить' : 'Скачать и установить')}</button></div>)}
  </>)}
  {tr(progress && <div className="engine-operation" role="status"><span><RefreshCw className={busy && !progress.paused ? 'spin' : ''} size={17}/>{tr(progress.paused?'На паузе':formatEnginePhase(progress.phase))}{engineProgressPercent(progress)!==undefined ? ` · ${enginePercentText(progress)}` : ''}</span><progress aria-label={tr("Прогресс загрузки")} aria-valuetext={enginePercentText(progress)} max={100} value={engineProgressPercent(progress)}/><div className="engine-progress-metrics">{progress.phase === 'Скачиваем движок' && <span>{tr("Скорость сети")}: {transferSize(progress.bytesPerSecond || 0)}{tr("/с")} · {tr("Получено по сети")}: {transferSize(progress.networkBytes)}</span>}{engineProgressMetrics(progress).map(row=><span key={row.label} className={row.currentFile?"engine-current-file":row.fileWork?"engine-file-work":undefined} title={row.currentFile?row.value:row.fileWork?progress.currentFile||undefined:undefined}>{tr(row.label)}{row.value?`: ${row.value}`:""}{row.fileProgress&&<progress className="engine-file-meter" aria-label={tr("Прогресс текущего файла")} aria-valuetext={row.fileProgress.text} max={100} value={row.fileProgress.value}/>}</span>)}</div><div className="engine-operation-actions"><EnginePauseButton paused={progress.paused} canPause={busy&&progress.canPause}/>{tr(busy && preview && ['Скачиваем движок','Собираем и проверяем файлы','Проверяем установленные файлы'].includes(progress.phase) && <button className="danger-text engine-stop-button" onClick={() => void invoke('engine_cancel')}><Square size={16}/>{tr("Остановить загрузку")}</button>)}</div></div>)}
  {tr(result && <><p role="status" style={{whiteSpace:'pre-line'}}>{result.split('\n').map(line=>tr(line)).join('\n')}</p><button className="primary" onClick={close}>{tr("Готово")}</button></>)}{tr(error && <p className="error-text" role="alert">{tr(error)}</p>)}
 </section></div>, document.querySelector('.workspace-main') || document.body))}

 </div>;
}

