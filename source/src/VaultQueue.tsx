import {useRef} from 'react';
import {EpicAuthError} from './EpicAuthError';
import {ArrowDown, ArrowUp, GripVertical, PackageOpen, Pause, Play, X} from 'lucide-react';
import {tr} from './i18n';
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
    canDefer?: boolean;
    canCancel?: boolean;
    controlPending?: 'defer' | 'cancel' | null;
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

export type ManualVaultTransfer = {item: {id: string}; plan: VaultPackage};
const queuePathKey = (path: string) => path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
export function isManualQueueTransfer(item: VaultQueueItem | undefined, manual: ManualVaultTransfer | null | undefined): boolean {
    if (!item || !manual || (item.status !== 'interrupted' && item.status !== 'needsAction')) return false;
    const {plan} = manual;
    return !!plan.id && !!plan.path && !!item.requestedVaultPath && item.packageId === plan.id
        && item.assetId === (plan.assetId || manual.item.id)
        && queuePathKey(plan.path) === `${queuePathKey(item.requestedVaultPath)}\\${plan.id.toLowerCase()}`;
}

const activeDragType = 'application/x-unreal-manager-active-queue';
type QueueDrag = {id: string; revision: number; active?: boolean};
export type DeferQueueTransfer = (id: string, beforeId: string | null, revision: number) => void;
function readActiveDrag(transfer: DataTransfer): QueueDrag | null {
    try {
        const value = JSON.parse(transfer.getData(activeDragType));
        return typeof value.id === 'string' && value.id.length <= 128 && Number.isSafeInteger(value.revision) && value.revision >= 0
            ? {id: value.id, revision: value.revision, active: true} : null;
    } catch { return null; }
}
const queueDropTarget = (element: Element | null) => element?.closest<HTMLElement>('.vault-queue-row[data-status="pending"], [data-queue-drop-end]');

export function QueueActiveTransfer({active, revision = 0, busy = false, onPause, onCancel, onDefer}: {
    active: VaultQueueItem;
    revision?: number;
    busy?: boolean;
    onPause?: (id: string, paused: boolean, revision: number) => void;
    onCancel?: (id: string, revision: number) => void;
    onDefer?: DeferQueueTransfer;
}) {
    const pointerDrag = useRef<{id: string; revision: number; pointerId: number} | null>(null);
    const waiting = busy || !!active.controlPending;
    const canDefer = !waiting && !!active.canDefer && !!onDefer;
    const waitingText = active.controlPending === 'defer' ? 'Возвращаем загрузку в очередь…' : active.controlPending === 'cancel' ? 'Останавливаем загрузку…' : 'Применяем действие…';
    return <article className="transfer-card vault-queue-active" data-queue-id={active.id} data-status={active.status} draggable={canDefer} aria-busy={waiting}
        onDragStart={event => {
            if (!canDefer) {event.preventDefault(); return;}
            event.dataTransfer.setData(activeDragType, JSON.stringify({id: active.id, revision}));
            event.dataTransfer.effectAllowed = 'move';
        }}>
        <div className="transfer-main"><div className="transfer-art"><PackageOpen size={28}/></div><div className="transfer-info"><h3>{active.title}</h3><p>UE {active.selectedEngine} · {tr(active.progress?.paused ? 'На паузе' : active.progress?.phase || 'Подготовка загрузки')}</p>
            <div className="transfer-meter"><progress max={100} value={active.progress?.total ? Math.min(100, Math.floor(active.progress.done / active.progress.total * 100)) : undefined}/><strong>{active.progress?.total ? `${Math.min(100, Math.floor(active.progress.done / active.progress.total * 100))}%` : '—'}</strong></div>
        </div><div className="vault-queue-active-actions">
            <span className="vault-queue-grip vault-queue-active-grip" role="img" aria-label={tr('Перетащить активную загрузку в очередь')} aria-disabled={!canDefer} title={tr('Перетащить активную загрузку в очередь')}
                onDragStart={event => {event.preventDefault(); event.stopPropagation();}}
                onPointerDown={event => {
                    if (!canDefer || event.button !== 0 || !event.isPrimary) return;
                    event.preventDefault(); event.stopPropagation();
                    pointerDrag.current = {id: active.id, revision, pointerId: event.pointerId};
                    event.currentTarget.setPointerCapture(event.pointerId);
                }}
                onPointerUp={event => {
                    const source = pointerDrag.current;
                    if (!source || source.pointerId !== event.pointerId) return;
                    pointerDrag.current = null;
                    event.preventDefault(); event.stopPropagation();
                    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
                    const target = queueDropTarget(event.currentTarget.ownerDocument.elementFromPoint(event.clientX, event.clientY));
                    if (canDefer && target?.closest('.vault-download-queue') && target.dataset.activeDropTarget === 'true') onDefer?.(source.id, target.dataset.queueId || null, source.revision);
                }}
                onPointerCancel={() => {pointerDrag.current = null;}}
                onLostPointerCapture={() => {pointerDrag.current = null;}}><GripVertical size={20}/></span>
            <fieldset className="vault-queue-transfer-controls" disabled={waiting} aria-label={tr('Управление активной загрузкой')}>
                <button disabled={!active.progress?.canPause || !onPause} onClick={() => onPause?.(active.id, !active.progress?.paused, revision)}>{active.progress?.paused ? <Play size={16}/> : <Pause size={16}/>} {tr(active.progress?.paused ? 'Продолжить' : 'Пауза')}</button>
                <button disabled={!active.canDefer || !onDefer} onClick={() => onDefer?.(active.id, null, revision)}><ArrowDown size={16}/>{tr('Вернуть в очередь')}</button>
                <button disabled={!active.canCancel || !onCancel} onClick={() => onCancel?.(active.id, revision)}><X size={16}/>{tr('Отменить загрузку')}</button>
            </fieldset>
        </div>{waiting && <p className="vault-queue-control-status" role="status">{tr(waitingText)}</p>}</div>
        {active.progress && <div className="transfer-metrics"><div><small>{tr('Обработано')}</small><b>{transferSize(active.progress.done)}{active.progress.total>0?` / ${transferSize(active.progress.total)}`:''}</b></div><div><small>{tr('Скорость сети')}</small><b>{transferSize(active.progress.bytesPerSecond)}{tr('/с')}</b></div><div><small>{tr('Получено по сети')}</small><b>{transferSize(active.progress.networkBytes)}</b></div><div><small>{tr('Файлы')}</small><b>{active.progress.filesDone} / {active.progress.filesTotal}</b></div></div>}
    </article>;
}

export function VaultQueue({queue, manualActive, busy, resumeBusy, error, onPaused, onReorder, onRemove, onRetry, onResume, onDefer}: {
    queue: VaultQueueSnapshot;
    manualActive?: ManualVaultTransfer | null;
    busy: boolean;
    resumeBusy: boolean;
    error: string;
    onPaused: (paused: boolean) => void;
    onReorder: (ids: string[], revision: number) => void;
    onRemove: (id: string, revision: number) => void;
    onRetry: (id: string, revision: number) => void;
    onResume: (packageId: string) => void;
    onDefer?: DeferQueueTransfer;
}) {
    const drag = useRef<QueueDrag | null>(null);
    const pointerDrag = useRef<{id: string; revision: number; pointerId: number} | null>(null);
    const active = queue.items.find(item => item.id === queue.activeId);
    const pending = queue.items.filter(item => item.status === 'pending' && item.id !== queue.activeId);
    const attention = queue.items.filter(item => item.id !== queue.activeId && item.status !== 'pending' && !isManualQueueTransfer(item, manualActive));
    const headIsManual = isManualQueueTransfer(queue.items[0], manualActive);
    const headNeedsAttention = !active && !headIsManual && (queue.items[0]?.status === 'interrupted' || queue.items[0]?.status === 'needsAction');
    const move = (id: string, delta: number) => {
        const ids = pending.map(item => item.id), from = ids.indexOf(id), to = from + delta;
        if (busy || from < 0 || to < 0 || to >= ids.length) return;
        [ids[from], ids[to]] = [ids[to], ids[from]];
        onReorder(ids, queue.revision);
    };
    const acceptsActive = !!active?.canDefer && !active.controlPending && !!onDefer;
    const canDeferBefore = (targetId: string | null) => {
        if (targetId === null) return true;
        const index = pending.findIndex(item => item.id === targetId);
        return index > 0 && !pending[index].controlPending && pending.slice(0, index).some(item => !item.controlPending);
    };
    const acceptsDrag = (transfer: DataTransfer, targetId: string | null) => !busy && (!!drag.current || acceptsActive && canDeferBefore(targetId) && Array.from(transfer.types).includes(activeDragType));
    const drop = (source: QueueDrag | null, targetId: string | null) => {
        if (busy || !source || source.id === targetId) return;
        if (source.active) {
            if (acceptsActive && source.id === active?.id && canDeferBefore(targetId) && (targetId === null || pending.some(item => item.id === targetId))) onDefer?.(source.id, targetId, source.revision);
            return;
        }
        const ids = pending.map(item => item.id), from = ids.indexOf(source.id), to = targetId === null ? ids.length : ids.indexOf(targetId);
        if (from < 0 || to < 0) return;
        if (pending[from].controlPending || targetId !== null && pending[to].controlPending) return;
        ids.splice(from, 1);
        ids.splice(to, 0, source.id);
        if (ids.every((id, index) => id === pending[index].id)) return;
        onReorder(ids, source.revision);
    };
    return <section className="downloads-section vault-download-queue" aria-label={tr('Очередь загрузок')}>
        <div className="section-head"><h3>{tr('Очередь загрузок')} <span className="download-count">{pending.length + attention.length}</span></h3>
            <button disabled={busy || !queue.items.length} onClick={() => onPaused(!queue.paused)}>{queue.paused ? <Play size={16}/> : <Pause size={16}/>} {tr(queue.paused ? 'Запустить очередь' : 'Приостановить очередь')}</button>
        </div>
        <p className="downloads-hint">{tr(queue.paused ? 'Очередь приостановлена' : headNeedsAttention ? 'Очередь требует внимания: продолжите, повторите или уберите первое задание.' : !active && (pending.length || headIsManual) ? 'Ожидает завершения текущей операции' : 'Очередь выполняется только пока открыт Unreal Manager.')}</p>
        <EpicAuthError error={error || queue.error || ''}/>
        <div className="vault-queue-list">{[...pending, ...attention].map(item => {
            const position = pending.findIndex(row => row.id === item.id), movable = position >= 0, controlling = !!item.controlPending;
            const savedPercent = movable && item.progress && Number.isFinite(item.progress.done) && Number.isFinite(item.progress.total) && item.progress.total > 0
                ? Math.max(0, Math.min(100, Math.floor(item.progress.done / item.progress.total * 100))) : undefined;
            return <article className="vault-queue-row" key={item.id} data-queue-id={item.id} data-status={item.status} data-active-drop-target={movable && acceptsActive && canDeferBefore(item.id)} aria-label={item.title} aria-busy={controlling} draggable={movable && !busy && !controlling}
                onDragStart={event => {if (!movable || busy || controlling) {event.preventDefault(); return;} drag.current = {id: item.id, revision: queue.revision}; event.dataTransfer.setData('application/x-unreal-manager-queue', item.id); event.dataTransfer.effectAllowed = 'move';}}
                onDragEnd={() => {drag.current = null;}}
                onDragOver={event => {if (movable && !controlling && acceptsDrag(event.dataTransfer, item.id)) {event.preventDefault(); event.dataTransfer.dropEffect = 'move';}}}
                onDrop={event => {event.preventDefault(); const source = drag.current || readActiveDrag(event.dataTransfer); drag.current = null; if (movable && !controlling) drop(source, item.id);}}>
                <span className="vault-queue-grip" role="img" aria-label={movable ? tr(`Перетащить: ${item.title}`) : tr('Требует внимания')} style={{touchAction: 'none'}}
                    onDragStart={event => {event.preventDefault(); event.stopPropagation();}}
                    onPointerDown={event => {
                        if (!movable || busy || controlling || event.button !== 0 || !event.isPrimary) return;
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
                        const target = queueDropTarget(event.currentTarget.ownerDocument.elementFromPoint(event.clientX, event.clientY));
                        if (target && event.currentTarget.closest('.vault-download-queue')?.contains(target)) drop(source, target.dataset.queueId || null);
                    }}
                    onPointerCancel={() => {pointerDrag.current = null;}}
                    onLostPointerCapture={() => {pointerDrag.current = null;}}>{movable ? <GripVertical size={18}/> : <PackageOpen size={18}/>}</span>
                <div className="vault-queue-info"><b>{movable ? `${position + 1}. ` : ''}{item.title}</b><p role={controlling ? 'status' : undefined}>UE {item.selectedEngine} · {tr(item.controlPending === 'cancel' ? 'Останавливаем загрузку…' : movable ? 'В очереди' : item.status === 'interrupted' ? 'Загрузка прервана' : 'Требует внимания')}{savedPercent !== undefined ? ` · ${savedPercent}%` : ''}</p><small>{item.requestedVaultPath}</small><EpicAuthError error={item.error || ''}/></div>
                <div className="vault-queue-actions">{movable ? <><button className="icon" disabled={busy || controlling || position === 0 || !!pending[position - 1]?.controlPending} aria-label={tr(`Выше: ${item.title}`)} onClick={() => move(item.id, -1)}><ArrowUp size={17}/></button><button className="icon" disabled={busy || controlling || position === pending.length - 1 || !!pending[position + 1]?.controlPending} aria-label={tr(`Ниже: ${item.title}`)} onClick={() => move(item.id, 1)}><ArrowDown size={17}/></button></> : item.status === 'interrupted' && item.packageId ? <button disabled={busy || resumeBusy || !!active || controlling} onClick={() => onResume(item.packageId!)}>{tr('Продолжить загрузку')}</button> : <button disabled={busy || controlling} onClick={() => onRetry(item.id, queue.revision)}>{tr('Повторить')}</button>}
                    <button className="icon" disabled={busy || controlling} aria-label={tr(`Убрать из очереди: ${item.title}`)} onClick={() => onRemove(item.id, queue.revision)}><X size={17}/></button>
                </div>
            </article>;
        })}</div>
        {(acceptsActive || pending.length > 1) && <div className="vault-queue-drop-end" data-queue-drop-end="true" data-active-drop-target={acceptsActive}
            onDragOver={event => {if (acceptsDrag(event.dataTransfer, null)) {event.preventDefault(); event.dataTransfer.dropEffect = 'move';}}}
            onDrop={event => {event.preventDefault(); const source = drag.current || readActiveDrag(event.dataTransfer); drag.current = null; drop(source, null);}}>
            <ArrowDown size={17}/><span>{tr('Перетащите сюда, чтобы добавить в конец очереди')}</span>
        </div>}
    </section>;
}
