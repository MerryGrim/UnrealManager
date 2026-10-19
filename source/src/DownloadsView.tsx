import {EngineResumeTasks} from './EngineResumeTasks';
import {VaultPauseButton} from './VaultPauseButton';
import {VaultResumeTasks,type VaultTask} from './VaultResumeTasks';
import {EngineTransferCard, type EngineTransfer} from './EngineTransferCard';
import { tr, getLocale } from './i18n';
import { engineVersion } from './lib/materialVersions';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { CheckCircle2, Download, PackageOpen, Square, X,RefreshCw } from 'lucide-react';
import { FabCover, type FabItem } from './EpicLibrary';
import type { VaultPackage } from './Vault';
import type { DownloadRecord } from './DownloadHistory';
export type TransferProgress = {
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
const size = (n: number) => n >= 1073741824 ? `${(n / 1073741824).toFixed(2)} ГБ` : `${(n / 1048576).toFixed(1)} МБ`;
const eta = (seconds: number) => seconds < 60 ? 'Меньше минуты' : seconds < 3600 ? `Около ${Math.ceil(seconds / 60)} мин` : `Около ${Math.ceil(seconds / 3600)} ч`;
export function DownloadsView({ engineDownload, active, progress, installing, records, error, historyError, onClear, onStop, onDetails, onUse, busy, available, pending=[],onResume=()=>{},onForget=async()=>{},restoring=false }: {
    restoring?:boolean;
    pending?:VaultTask[];
    onResume?:(id:string)=>void;
    onForget?:(id:string)=>Promise<void>;
    engineDownload?: {progress:EngineTransfer;onOpen:()=>void} | null;
    active: {
        item: FabItem;
        plan: VaultPackage;
    } | null;
    progress: TransferProgress | null;
    installing: boolean;
    records: DownloadRecord[];
    error: string;
    historyError: string;
    onClear: (id?: string) => void;
    onStop: () => void;
    onDetails: () => void;
    onUse: (item: VaultPackage) => void;
    busy: boolean;
    available: VaultPackage[];
}) {
    const [covers, setCovers] = useState<FabItem[]>([]);
    useEffect(() => { let alive = true; invoke<{
        items: FabItem[];
    } | null>('fab_cached_library').then(c => { if (alive)
        setCovers(c?.items || []); }).catch(() => { }); return () => { alive = false; }; }, []);
    const pct = progress ? Math.min(100, Math.max(0, Math.floor(progress.done / Math.max(1, progress.total) * 100))) : 0;
    const cover = (item: VaultPackage) => covers.find(c => c.id === item.assetId || c.title === item.title);
    return <div className="content downloads-content">
  <section className="downloads-section"><div className="section-head"><h3>{tr("Текущие ")}<span className="download-count">{tr((active ? 1 : 0) + (engineDownload ? 1 : 0))}</span></h3></div>
   {restoring&&<p className="library-loading" role="status"><RefreshCw className="spin" size={18}/>{tr('Восстанавливаем загрузку…')}</p>}
   {engineDownload && <EngineTransferCard progress={engineDownload.progress} onOpen={engineDownload.onOpen}/>}
   {tr(active ? <article className="transfer-card vault-transfer-card"><div className="transfer-main"><div className="transfer-art"><FabCover item={active.item}/></div><div className="transfer-info"><h3>{active.item.title}</h3><p>{tr("UE ")}{tr(active.plan.engines.map(engineVersion).join(', '))}{tr(" · ")}{tr(installing ? 'Установка' : progress?.paused?'На паузе':progress?.phase || 'Подготовка загрузки')}</p><div className="transfer-meter"><progress aria-label={tr("Прогресс загрузки")} max={100} value={pct}/><strong>{tr(pct)}{tr("%")}</strong></div></div><div className="transfer-buttons">{!installing&&<VaultPauseButton paused={progress?.paused} canPause={progress?.canPause}/>}<button onClick={onDetails}>{tr("Подробнее")}</button><button disabled={installing} onClick={onStop}><Square size={15}/>{tr("Остановить")}</button></div></div>
   <div className="transfer-metrics"><div><small>{tr("Подготовлено")}</small><b>{tr(size(progress?.done || 0))}{tr(" / ")}{tr(size(progress?.total || active.plan.bytes))}</b></div><div><small>{tr("Скорость сети")}</small><b>{tr(size(progress?.bytesPerSecond || 0))}{tr("/с")}</b></div><div><small>{tr("Получено по сети")}</small><b>{tr(size(progress?.networkBytes || 0))}</b></div><div><small>{tr(installing ? 'Состояние' : 'Осталось')}</small><b>{tr(installing ? 'Устанавливаем…' : progress && progress.bytesPerSecond > 0 && active.plan.downloadBytes > (progress.networkBytes || 0) ? eta((active.plan.downloadBytes - progress.networkBytes) / progress.bytesPerSecond) : '—')}</b></div></div></article>
            : !engineDownload && <div className="downloads-idle"><Download size={27}/><div><b>{tr("Нет активных загрузок")}</b><p>{tr("Выберите материал в библиотеке, чтобы начать скачивание.")}</p></div></div>)}
   {tr(error && <p className="error-text" role="alert">{tr(error)}</p>)}
  </section>
  <VaultResumeTasks tasks={pending.filter(t=>t.id!==active?.plan.id)} busy={busy} covers={covers} onResume={onResume} onForget={onForget}/>
  {!engineDownload&&<EngineResumeTasks busy={busy} onResume={id=>window.dispatchEvent(new CustomEvent('um-resume-engine',{detail:id}))} refreshKey={engineDownload}/> }
  <section className="downloads-section"><div className="section-head"><h3>{tr("Загруженные ")}<span className="download-count">{tr(records.length)}</span></h3><button disabled={!records.length} onClick={() => onClear()}>{tr("Очистить список")}</button></div><p className="downloads-hint">{tr("Очистка списка не удаляет скачанные файлы.")}</p>
   {tr(historyError && <p role="alert" className="error-text">{tr(historyError)}</p>)}
   {tr(records.length ? <div className="completed-downloads">{tr(records.map(({ item, completedAt }) => { const art = cover(item); return <article className="completed-download" key={item.id}><div className="transfer-art">{tr(art ? <FabCover item={art}/> : <PackageOpen size={26}/>)}</div><div className="completed-info"><b>{item.title}</b><p>{tr("UE ")}{tr(item.engines.map(engineVersion).join(', ') || '—')}{tr(" · ")}{tr(size(item.bytes))}</p></div><div className="completed-state"><span><CheckCircle2 size={15}/>{tr("Загружено")}</span><small>{tr(completedAt ? new Date(completedAt).toLocaleString(getLocale()) : '')}</small></div><button disabled={busy || !available.some(p => p.id === item.id)} onClick={() => onUse(item)}>{tr(item.kind === 'Plugin' ? 'К плагинам' : item.kind === 'Project' ? 'Создать проект' : 'Добавить в проект')}</button><button className="icon" aria-label={tr(`Убрать из списка: ${item.title}`)} title={tr("Убрать из списка")} onClick={() => onClear(item.id)}><X size={17}/></button></article>; }))}</div> : <div className="downloads-empty">{tr("Список пуст. Здесь появятся завершённые загрузки.")}</div>)}
  </section>
 </div>;
}
