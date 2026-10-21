import {useRef} from 'react';
import {EpicAuthError} from './EpicAuthError';
import {ArrowDown, ArrowUp, GripVertical, PackageOpen, Pause, Play, X} from 'lucide-react';
import {tr} from './i18n';
import {VaultPauseButton} from './VaultPauseButton';
import {transferSize} from './EngineTransferCard';
import type {TransferProgress} from './DownloadsView';
import type {VaultPackage} from './Vault';

export type VaultQueueItem = {
    id: string;
    assetId: string;
    artifactId: string;
    selectedEngine: string;
    title: string;
    requestedVaultPath: string;
    status: 'pending' | 'preparing' | 'downloading' | 'needsAction' | 'interrupted';
    packageId: string | null;
    error: string | null;
    progress: TransferProgress | null;
};
export type VaultQueueSnapshot = {
    revision: number;
    paused: boolean;
    activeId: string | null;
    items: VaultQueueItem[];
    error: string | null;
    completed?: VaultPackage | null;
};
export const emptyVaultQueue: VaultQueueSnapshot = {revision: 0, paused: true, activeId: null, items: [], error: null};

export function QueueActiveTransfer({active}: {active: VaultQueueItem}) {
    return <article className="transfer-card vault-queue-active" data-queue-id={active.id} data-status={active.status}>
        <div className="transfer-main"><div className="transfer-art"><PackageOpen size={28}/></div><div className="transfer-info"><h3>{active.title}</h3><p>UE {active.selectedEngine} · {tr(active.progress?.paused ? 'На паузе' : active.progress?.phase || 'Подготовка загрузки')}</p>
            <div className="transfer-meter"><progress max={100} value={active.progress?.total ? Math.min(100, Math.floor(active.progress.done / active.progress.total * 100)) : undefined}/><strong>{active.progress?.total ? `${Math.min(100, Math.floor(active.progress.done / active.progress.total * 100))}%` : '—'}</strong></div>
        </div><VaultPauseButton paused={active.progress?.paused} canPause={active.progress?.canPause}/></div>
        {active.progress && <div className="transfer-metrics"><div><small>{tr('Обработано')}</small><b>{transferSize(active.progress.done)}{active.progress.total>0?` / ${transferSize(active.progress.total)}`:''}</b></div><div><small>{tr('Скорость сети')}</small><b>{transferSize(active.progress.bytesPerSecond)}{tr('/с')}</b></div><div><small>{tr('Получено по сети')}</small><b>{transferSize(active.progress.networkBytes)}</b></div><div><small>{tr('Файлы')}</small><b>{active.progress.filesDone} / {active.progress.filesTotal}</b></div></div>}
    </article>;
}

export function VaultQueue({queue, busy, resumeBusy, error, onPaused, onReorder, onRemove, onRetry, onResume}: {
    queue: VaultQueueSnapshot;
    busy: boolean;
    resumeBusy: boolean;
    error: string;
    onPaused: (paused: boolean) => void;
    onReorder: (ids: string[], revision: number) => void;
    onRemove: (id: string, revision: number) => void;
    onRetry: (id: string, revision: number) => void;
    onResume: (packageId: string) => void;
}) {
    const drag = useRef<{id: string; revision: number} | null>(null);
    const pointerDrag = useRef<{id: string; revision: number; pointerId: number} | null>(null);
    const active = queue.items.find(item => item.id === queue.activeId);
    const pending = queue.items.filter(item => item.status === 'pending' && item.id !== queue.activeId);
    const attention = queue.items.filter(item => item.id !== queue.activeId && item.status !== 'pending');
    const headNeedsAttention = !active && (queue.items[0]?.status === 'interrupted' || queue.items[0]?.status === 'needsAction');
    const move = (id: string, delta: number) => {
        const ids = pending.map(item => item.id), from = ids.indexOf(id), to = from + delta;
        if (busy || from < 0 || to < 0 || to >= ids.length) return;
        [ids[from], ids[to]] = [ids[to], ids[from]];
        onReorder(ids, queue.revision);
    };
    const drop = (source: {id: string; revision: number} | null, targetId: string) => {
        if (busy || !source || source.id === targetId) return;
        const ids = pending.map(item => item.id), from = ids.indexOf(source.id), to = ids.indexOf(targetId);
        if (from < 0 || to < 0) return;
        ids.splice(from, 1);
        ids.splice(to, 0, source.id);
        onReorder(ids, source.revision);
    };
    return <section className="downloads-section vault-download-queue" aria-label={tr('Очередь загрузок')}>
        <div className="section-head"><h3>{tr('Очередь загрузок')} <span className="download-count">{pending.length + attention.length}</span></h3>
            <button disabled={busy || !queue.items.length} onClick={() => onPaused(!queue.paused)}>{queue.paused ? <Play size={16}/> : <Pause size={16}/>} {tr(queue.paused ? 'Запустить очередь' : 'Приостановить очередь')}</button>
        </div>
        <p className="downloads-hint">{tr(queue.paused ? 'Очередь приостановлена' : headNeedsAttention ? 'Очередь требует внимания: продолжите, повторите или уберите первое задание.' : !active && pending.length ? 'Ожидает завершения текущей операции' : 'Очередь выполняется только пока открыт Unreal Manager.')}</p>
        <EpicAuthError error={error || queue.error || ''}/>
        <div className="vault-queue-list">{[...pending, ...attention].map(item => {
            const position = pending.findIndex(row => row.id === item.id), movable = position >= 0;
            return <article className="vault-queue-row" key={item.id} data-queue-id={item.id} data-status={item.status} aria-label={item.title} draggable={movable && !busy}
                onDragStart={event => {if (!movable || busy) {event.preventDefault(); return;} drag.current = {id: item.id, revision: queue.revision}; event.dataTransfer.setData('application/x-unreal-manager-queue', item.id); event.dataTransfer.effectAllowed = 'move';}}
                onDragEnd={() => {drag.current = null;}}
                onDragOver={event => {if (movable && drag.current && !busy) {event.preventDefault(); event.dataTransfer.dropEffect = 'move';}}}
                onDrop={event => {event.preventDefault(); const source = drag.current; drag.current = null; if (movable) drop(source, item.id);}}>
                <span className="vault-queue-grip" role="img" aria-label={movable ? tr(`Перетащить: ${item.title}`) : tr('Требует внимания')} style={{touchAction: 'none'}}
                    onDragStart={event => {event.preventDefault(); event.stopPropagation();}}
                    onPointerDown={event => {
                        if (!movable || busy || event.button !== 0 || !event.isPrimary) return;
                        event.preventDefault(); event.stopPropagation();
                        drag.current = null;
                        pointerDrag.current = {id: item.id, revision: queue.revision, pointerId: event.pointerId};
                        event.currentTarget.setPointerCapture(event.pointerId);
                    }}
                    onPointerUp={event => {
                        const source = pointerDrag.current;
                        if (!source || source.pointerId !== event.pointerId) return;
                        pointerDrag.current = null;
                        event.preventDefault(); event.stopPropagation();
                        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
                        const target = event.currentTarget.ownerDocument.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('.vault-queue-row');
                        if (target && event.currentTarget.closest('.vault-download-queue')?.contains(target)) drop(source, target.dataset.queueId || '');
                    }}
                    onPointerCancel={() => {pointerDrag.current = null;}}
                    onLostPointerCapture={() => {pointerDrag.current = null;}}>{movable ? <GripVertical size={18}/> : <PackageOpen size={18}/>}</span>
                <div className="vault-queue-info"><b>{movable ? `${position + 1}. ` : ''}{item.title}</b><p>UE {item.selectedEngine} · {tr(movable ? 'В очереди' : item.status === 'interrupted' ? 'Загрузка прервана' : 'Требует внимания')}</p><small>{item.requestedVaultPath}</small><EpicAuthError error={item.error || ''}/></div>
                <div className="vault-queue-actions">{movable ? <><button className="icon" disabled={busy || position === 0} aria-label={tr(`Выше: ${item.title}`)} onClick={() => move(item.id, -1)}><ArrowUp size={17}/></button><button className="icon" disabled={busy || position === pending.length - 1} aria-label={tr(`Ниже: ${item.title}`)} onClick={() => move(item.id, 1)}><ArrowDown size={17}/></button></> : item.status === 'interrupted' && item.packageId ? <button disabled={busy || resumeBusy || !!active} onClick={() => onResume(item.packageId!)}>{tr('Продолжить загрузку')}</button> : <button disabled={busy} onClick={() => onRetry(item.id, queue.revision)}>{tr('Повторить')}</button>}
                    <button className="icon" disabled={busy} aria-label={tr(`Убрать из очереди: ${item.title}`)} onClick={() => onRemove(item.id, queue.revision)}><X size={17}/></button>
                </div>
            </article>;
        })}</div>
    </section>;
}
