import {RefreshCw} from 'lucide-react';
import {tr} from './i18n';
import {EnginePermissionError} from './EnginePermissionError';
import {enginePercentText, engineProgressMetrics, engineProgressPercent, formatEnginePhase, type EngineProgress} from './EngineTransferCard';

export function EngineRecoveryProgress({path,progress,busy,result,error,onClose}:{
    path:string;progress:EngineProgress|null;busy:boolean;result:string;error:string;onClose:()=>void;
}) {
    return <section className="card engine-recovery-progress" aria-label={tr('Восстанавливаем исходные файлы')} aria-busy={busy}>
        <h3>{tr('Восстанавливаем исходные файлы')}</h3>
        <p className="project-delete-path">{path}</p>
        {progress && <div className="engine-operation" role="status">
            <span><RefreshCw size={17} className={busy?'spin':''}/>{formatEnginePhase(progress.phase)}{engineProgressPercent(progress)!==undefined?` · ${enginePercentText(progress)}`:''}</span>
            <progress aria-label={tr('Прогресс загрузки')} aria-valuetext={enginePercentText(progress)} max={100} value={engineProgressPercent(progress)}/>
            <div className="engine-progress-metrics">{engineProgressMetrics(progress).map(row=><span key={row.label} className={row.currentFile?'engine-current-file':row.fileWork?'engine-file-work':undefined} title={row.currentFile?row.value:undefined}>
                {tr(row.label)}{row.value?`: ${row.value}`:''}
                {row.fileProgress && <progress className="engine-file-meter" aria-label={tr('Прогресс текущего файла')} aria-valuetext={row.fileProgress.text} max={100} value={row.fileProgress.value}/>}
            </span>)}</div>
        </div>}
        {error && <EnginePermissionError error={error}/>}
        {result && <p role="status">{tr(result)}</p>}
        {!busy && <button onClick={onClose}>{tr('Закрыть')}</button>}
    </section>;
}
