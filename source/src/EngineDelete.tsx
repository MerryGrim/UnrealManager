import { tr } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Channel, invoke } from '@tauri-apps/api/core';
import { RefreshCw, FileX2 } from 'lucide-react';
import {type EngineProgress, formatEnginePhase, enginePercentText, engineProgressPercent, engineProgressMetrics} from './EngineTransferCard';
type DeleteProgress=EngineProgress & {processedBytes:number;totalBytes:number;filesDone:number;filesTotal:number};
export function EngineDelete({ path, onClose, onDeleted }: {
    path: string;
    onClose: () => void;
    onDeleted: () => Promise<void>;
}) {
    const [preview, setPreview] = useState<{path: string; version: string} | null>(null);
    const [error, setError] = useState(''), [busy, setBusy] = useState(false), [result, setResult] = useState('');
    const [progress, setProgress] = useState<DeleteProgress | null>(null);
    const lock = useRef(false), cancel = useRef<HTMLButtonElement>(null);
    useEffect(() => {
        let gone = false;
        cancel.current?.focus();
        void invoke<{path: string; version: string}>('engine_delete_preview', {path})
            .then(value => { if (!gone) setPreview(value); })
            .catch(e => { if (!gone) setError(String(e)); });
        return () => { gone = true; };
    }, [path]);
    const remove = async () => {
        if (!preview || lock.current || result) return;
        lock.current = true;
        setBusy(true);
        setError('');
        setProgress(null);
        let sampleTime=performance.now(),sampleBytes=0,samplePhase='',speed=0;
        const onProgress=new Channel<DeleteProgress>();
        onProgress.onmessage=p=>{
            const now=performance.now();
            if(p.phase!==samplePhase){sampleTime=now;sampleBytes=p.processedBytes;samplePhase=p.phase;speed=0;}
            if(now-sampleTime>=500){speed=Math.max(0,p.processedBytes-sampleBytes)*1000/(now-sampleTime);sampleTime=now;sampleBytes=p.processedBytes;}
            setProgress({...p,processingBytesPerSecond:speed});
        };
        try {
            const message = await invoke<string>('engine_delete', {path: preview.path, expectedVersion: preview.version,onProgress});
            setResult(message);
            await onDeleted();
        } catch (e) { setError(String(e)); }
        finally { setBusy(false); lock.current = false; }
    };
    return createPortal(<div className="modal-backdrop" onKeyDown={e => {
        if (e.key === 'Escape' && !busy) { e.preventDefault(); onClose(); }
        if (e.key === 'Tab') {
            const controls = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
            const index = controls.indexOf(document.activeElement as HTMLButtonElement);
            e.preventDefault();
            controls[(index + (e.shiftKey ? controls.length - 1 : 1)) % controls.length]?.focus();
        }
    }}><section className="engine-wizard engine-delete-dialog" role="alertdialog" aria-modal="true" aria-label={tr("Удаление движка")}>
        <h2>{tr("Удаление Unreal Engine ")}{preview?.version}</h2><p className="project-delete-path">{path}</p>
        {!result && <><p>{tr("Будет удалена вся папка движка, включая установленные в неё плагины. Проекты вне этой папки и Vault сохранятся.")}</p><p className="error-text">{tr("Файлы будут стёрты без возможности восстановления из корзины.")}</p></>}
        {!preview && !error && <p role="status">{tr("Проверяем установку…")}</p>}
        {busy && !result && (progress?<div className="engine-operation" role="status"><span><RefreshCw size={17} className="spin"/>{formatEnginePhase(progress.phase)}{engineProgressPercent(progress)!==undefined?` · ${enginePercentText(progress)}`:''}</span><progress aria-label={tr("Прогресс загрузки")} aria-valuetext={enginePercentText(progress)} max={100} value={engineProgressPercent(progress)}/><div className="engine-progress-metrics">{engineProgressMetrics(progress).map(row=><span key={row.label} className={row.currentFile?"engine-current-file":undefined} title={row.currentFile?row.value:undefined}>{tr(row.label)}: {row.value}</span>)}</div></div>:<p role="status"><RefreshCw size={17} className="spin"/> {tr("Удаляем…")}</p>)}
        {error && <p role="alert" className="error-text">{tr(error)}</p>}
        {result && <p role="status">{tr(result)}</p>}
        <div className="wizard-footer">
            <button ref={cancel} disabled={busy} onClick={onClose}>{tr(result ? 'Закрыть' : 'Отмена')}</button>
            {!result && <button className="danger-button" disabled={!preview || busy} onClick={() => void remove()}><FileX2 size={17}/>{tr(busy ? 'Удаляем…' : 'Удалить навсегда')}</button>}
        </div>
    </section></div>, document.body);
}
