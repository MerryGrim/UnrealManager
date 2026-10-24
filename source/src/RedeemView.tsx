import { Channel, invoke } from '@tauri-apps/api/core';
import { CheckCircle2, ExternalLink, FileUp, KeyRound, RefreshCw, Square, X } from 'lucide-react';
import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { EpicAuthError, isEpicAuthRequired } from './EpicAuthError';
import { getLocale, tr, useLanguage } from './i18n';
import { ModalFocusScope } from './ModalFocusScope';
import { parseRedemptionBatch, REDEMPTION_BATCH_LIMITS, type RedemptionBatchIssue } from './redemptionBatch';

export type RedemptionStatus = 'pending' | 'checking' | 'ready' | 'activating' | 'success' | 'failed' | 'unknown' | 'notAttempted' | 'requiresOfficial';
export type RedemptionReason = 'invalidInput' | 'duplicate' | 'notFound' | 'alreadyUsed' | 'alreadyOwned' | 'expired' | 'regionRestricted' | 'authRequired' | 'accountChanged' | 'previewExpired' | 'requiresOfficial' | 'network' | 'service' | 'unknown' | 'cancelled' | 'rateLimited';
type DiagnosticKind = 'missing' | 'null' | 'array' | 'object' | 'string' | 'number' | 'boolean' | 'other';
export type RedemptionDiagnostic = {
    operation: 'account' | 'evaluate' | 'lock' | 'redeem' | 'unlock';
    stage: 'origin' | 'headers' | 'request' | 'http' | 'bodyShape' | 'bodyRead' | 'bodySize' | 'json' | 'accountShape' | 'resultShape' | 'productShape' | 'identifier' | 'echo';
    httpStatus?: number;
    productKind?: DiagnosticKind; eulaIdsKind?: DiagnosticKind; namespaceKind?: DiagnosticKind;
    entitlementKind?: DiagnosticKind; titleKind?: DiagnosticKind; codeUseIdKind?: DiagnosticKind;
    hasErrors?: boolean; success?: boolean;
    serviceCode?: string;
};
export type RedemptionItem = {
    id: string;
    index: number;
    maskedKey: string;
    status: RedemptionStatus;
    title: string;
    reason: RedemptionReason | null;
    diagnostic?: RedemptionDiagnostic;
};
export type RedemptionSnapshot = {
    batchId: string;
    accountName: string;
    phase: 'preparing' | 'ready' | 'activating' | 'completed' | 'stopped';
    items: RedemptionItem[];
    expiresAt?: number;
    waitSeconds?: number;
    waitReason?: 'pacing' | 'rateLimited' | 'batch' | 'beforeActivation';
    preparationStage?: 'account' | 'session' | 'connection' | 'keys';
};
type OfficialResult = { status: 'ready' | 'success' | 'failed' | 'unknown'; title: string; errorCode: string };
type Work = 'prepare' | 'activate' | 'official' | 'discard' | null;
type UiError = 'auth' | 'busy' | 'operation' | 'file' | 'invalidInput' | 'checkingBusy' | 'previewExpired' | 'activationBusy' | null;

const statusLabels: Record<RedemptionStatus, string> = {
    pending: 'В очереди', checking: 'Проверяем ключ…', ready: 'Готов к активации', activating: 'Активируем…',
    success: 'Успешно', failed: 'Неуспешно', unknown: 'Результат не подтверждён', notAttempted: 'Не обработан', requiresOfficial: 'Требуется сайт Epic',
};
const preparationLabels = {
    account: 'Проверяем аккаунт Epic…', session: 'Подготавливаем вход Epic…',
    connection: 'Подключаемся к Epic…', keys: 'Проверяем ключи…',
};
const reasonLabels: Record<RedemptionReason, string> = {
    invalidInput: 'Проверьте символы ключа.', duplicate: 'Повтор ключа в этом списке.', notFound: 'Epic не нашёл этот ключ.',
    alreadyUsed: 'Этот ключ уже использован.', alreadyOwned: 'Продукт уже есть в аккаунте.', expired: 'Срок действия ключа истёк.',
    regionRestricted: 'Ключ недоступен для региона аккаунта.', authRequired: 'Войдите в Epic заново.', accountChanged: 'Аккаунт изменился. Проверьте ключи заново.',
    previewExpired: 'Проверка устарела. Проверьте ключи заново.', requiresOfficial: 'Продолжите на официальном сайте Epic.',
    network: 'Не удалось связаться с Epic. Проверьте сеть.', service: 'Epic временно не может обработать запрос.',
    rateLimited: 'Epic ограничил частоту запросов.',
    unknown: 'Не повторяйте активацию автоматически. Проверьте результат у Epic.', cancelled: 'Обработка остановлена пользователем.',
};
const errorLabels = {
    busy: 'Сначала закройте окно Epic Games.', operation: 'Не удалось выполнить операцию. Попробуйте позже или откройте официальный сайт Epic.',
    file: 'Не удалось прочитать TXT. Выберите другой файл.', invalidInput: 'Некорректный список ключей.', checkingBusy: 'Проверка ключей уже выполняется.',
    previewExpired: 'Подтверждение ключей устарело. Проверьте ключи заново.', activationBusy: 'Сначала остановите текущую активацию.',
};
/** Never format arbitrary service text or serialize an untrusted diagnostic. */
function DiagnosticDetails({ value, maskedKey }: { value: unknown; maskedKey: string }) {
    if (!value || typeof value !== 'object') return null;
    const data = value as Record<string, unknown>;
    const operations = ['account', 'evaluate', 'lock', 'redeem', 'unlock'];
    const stages = ['origin', 'headers', 'request', 'http', 'bodyShape', 'bodyRead', 'bodySize', 'json', 'accountShape', 'resultShape', 'productShape', 'identifier', 'echo'];
    const kinds = ['missing', 'null', 'array', 'object', 'string', 'number', 'boolean', 'other'];
    if (typeof data.operation !== 'string' || !operations.includes(data.operation) || typeof data.stage !== 'string' || !stages.includes(data.stage)) return null;
    const parts = [data.operation, data.stage];
    if (typeof data.httpStatus === 'number' && Number.isInteger(data.httpStatus) && data.httpStatus >= 100 && data.httpStatus <= 599) parts.push(`HTTP ${data.httpStatus}`);
    for (const field of ['productKind', 'eulaIdsKind', 'namespaceKind', 'entitlementKind', 'titleKind', 'codeUseIdKind']) {
        if (typeof data[field] === 'string' && kinds.includes(data[field] as string)) parts.push(`${field}: ${data[field]}`);
    }
    for (const field of ['hasErrors', 'success']) if (typeof data[field] === 'boolean') parts.push(`${field}: ${data[field] ? 'true' : 'false'}`);
    const suffix = maskedKey.replace(/[^a-z0-9]/gi, '').toLowerCase();
    if (typeof data.serviceCode === 'string' && /^errors\.com\.epicgames\.[a-z_.]{1,160}$/.test(data.serviceCode)
        && (!suffix || !data.serviceCode.replace(/[^a-z0-9]/g, '').includes(suffix))) parts.push(`serviceCode: ${data.serviceCode}`);
    return <details className="redemption-diagnostic"><summary>{tr('Детали проверки')}</summary><code>{parts.join(' · ')}</code></details>;
}
const classifyError = (failure: unknown): UiError => {
    if (isEpicAuthRequired(failure)) return 'auth';
    const message = failure instanceof Error ? failure.message : failure;
    return (Object.keys(errorLabels) as (keyof typeof errorLabels)[]).find(key => errorLabels[key] === message) ?? 'operation';
};
function validSnapshot(value: RedemptionSnapshot, expectedMasks: string[]): boolean {
    return !!value && typeof value.batchId === 'string' && value.batchId.length > 0 && value.batchId.length <= 128
        && typeof value.accountName === 'string' && ['preparing', 'ready', 'activating', 'completed', 'stopped'].includes(value.phase)
        && (value.waitSeconds === undefined || Number.isInteger(value.waitSeconds) && value.waitSeconds >= 0 && value.waitSeconds <= 3600)
        && (value.waitReason === undefined || ['pacing', 'rateLimited', 'batch', 'beforeActivation'].includes(value.waitReason))
        && (value.preparationStage === undefined || Object.hasOwn(preparationLabels, value.preparationStage))
        && Array.isArray(value.items) && value.items.length === expectedMasks.length && value.items.length <= REDEMPTION_BATCH_LIMITS.maxKeys
        && value.items.every(item => !!item && typeof item.id === 'string' && item.id.length > 0 && Number.isInteger(item.index)
            && item.index >= 0 && item.index < expectedMasks.length && item.maskedKey === expectedMasks[item.index]
            && Object.hasOwn(statusLabels, item.status) && typeof item.title === 'string'
            && (item.reason == null || Object.hasOwn(reasonLabels, item.reason)))
        && new Set(value.items.map(item => item.id)).size === value.items.length
        && new Set(value.items.map(item => item.index)).size === value.items.length;
}
const issueLabel = (issue: RedemptionBatchIssue) => {
    switch (issue.code) {
        case 'empty': return 'Введите хотя бы один ключ.';
        case 'inputTooLarge': return 'Размер текста или TXT не должен превышать 64 КиБ.';
        case 'tooManyRows': return 'Не более 256 строк.';
        case 'tooManyKeys': return 'Не более 100 уникальных ключей.';
        case 'keyTooLong': return `Строка ${issue.line}: ключ длиннее 128 символов.`;
        case 'invalidCharacters': return `Строка ${issue.line}: допустимы только латинские буквы, цифры и дефисы.`;
    }
};

export function RedeemView({ account, active, onLibrary, onActivated }: {
    account: { signedIn: boolean; displayName: string | null };
    active: boolean;
    onLibrary: () => void;
    onActivated: () => void;
}) {
    useLanguage();
    const [draft, setDraft] = useState('');
    const [issues, setIssues] = useState<RedemptionBatchIssue[]>([]);
    const [snapshot, setSnapshot] = useState<RedemptionSnapshot | null>(null);
    const [work, setWork] = useState<Work>(null);
    const [error, setError] = useState<UiError>(null);
    const [stopping, setStopping] = useState(false);
    const [confirm, setConfirm] = useState(false);
    const [acknowledged, setAcknowledged] = useState(false);
    const [official, setOfficial] = useState<OfficialResult | null>(null);
    const [lastOperation, setLastOperation] = useState<'check' | 'activate'>('check');
    const [stoppedBeforeAcceptance, setStoppedBeforeAcceptance] = useState(false);
    const [preparationStage, setPreparationStage] = useState<RedemptionSnapshot['preparationStage']>();
    const field = useRef<HTMLTextAreaElement>(null), fileInput = useRef<HTMLInputElement>(null), heading = useRef<HTMLHeadingElement>(null);
    const mounted = useRef(true), lock = useRef(false), workRef = useRef<Work>(null), current = useRef<RedemptionSnapshot | null>(null);
    const sequence = useRef(0), importSequence = useRef(0), cancelWanted = useRef(false), cancelSent = useRef<string | null>(null), resetAfterWork = useRef(false);
    const activeProgressToken = useRef<number | null>(null);
    const accountIdentity = `${account.signedIn}:${account.displayName ?? ''}`;
    const previousAccount = useRef(accountIdentity);
    const activatedBatch = useRef<string | null>(null);
    const expectedMasks = useRef<string[]>([]);
    const nativeAccepted = useRef(false);
    const busy = work !== null;
    const number = (value: number) => new Intl.NumberFormat(getLocale()).format(value);

    const clearDraft = () => {
        importSequence.current += 1;
        setDraft('');
        setIssues([]);
        if (fileInput.current) fileInput.current.value = '';
    };
    const discard = async (id: string) => { await invoke('epic_redeem_batch_discard', { batchId: id }); };
    const sendCancel = async () => {
        const id = current.current?.batchId;
        if (!id || cancelSent.current === id) return;
        cancelSent.current = id;
        try { await invoke<boolean>('epic_redeem_batch_cancel', { batchId: id }); }
        catch (failure) { if (mounted.current && !resetAfterWork.current) setError(classifyError(failure)); }
    };
    const privacyReset = () => {
        clearDraft();
        setConfirm(false);
        setAcknowledged(false);
        setSnapshot(null);
        setOfficial(null);
        setStoppedBeforeAcceptance(false);
        setPreparationStage(undefined);
        setError(null);
        if (workRef.current) {
            resetAfterWork.current = true;
            cancelWanted.current = true;
            setStopping(true);
            void sendCancel();
        } else if (current.current) {
            sequence.current += 1; // Hidden/account-invalidated previews must not be revived by their expiry channel.
            const id = current.current.batchId;
            void discard(id).then(() => { if (current.current?.batchId === id) current.current = null; })
                .catch(() => { if (mounted.current) setError('operation'); });
        }
    };
    useEffect(() => {
        const changedAccount = previousAccount.current !== accountIdentity;
        previousAccount.current = accountIdentity;
        if (!active || changedAccount) privacyReset();
    }, [active, accountIdentity]);
    useEffect(() => {
        if (active && snapshot) heading.current?.focus({ preventScroll: true });
    }, [active, snapshot?.batchId]);
    useEffect(() => {
        if (active && !snapshot && !work) field.current?.focus({ preventScroll: true });
    }, [active, !!snapshot, work]);
    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            importSequence.current += 1;
            cancelWanted.current = true;
            resetAfterWork.current = true;
            if (workRef.current) void sendCancel();
            else if (current.current) void discard(current.current.batchId).catch(() => {});
        };
    }, []);

    const accept = (value: RedemptionSnapshot, token: number, expectedId?: string, progressOnly = false) => {
        if (sequence.current !== token) return false;
        const idleExpiry = progressOnly && activeProgressToken.current !== token;
        if (idleExpiry) {
            const previous = current.current;
            const expiresAt = previous?.expiresAt;
            // A stored prepare channel may emit its one-shot expiry after the RPC has completed.
            // Every other late event remains closed, including events before the actual deadline.
            if (!mounted.current || resetAfterWork.current || workRef.current !== null || previous?.phase !== 'ready'
                || typeof expiresAt !== 'number' || !Number.isSafeInteger(expiresAt) || expiresAt <= 0 || Date.now() / 1000 < expiresAt
                || !validSnapshot(value, expectedMasks.current) || value.batchId !== previous.batchId || value.phase !== 'stopped'
                || !value.items.some(item => item.reason === 'previewExpired')) return false;
        }
        if (!validSnapshot(value, expectedMasks.current)) {
            if (mounted.current && !resetAfterWork.current) setError('operation');
            return false;
        }
        if (expectedId && value.batchId !== expectedId) return false;
        if (current.current && current.current.batchId !== value.batchId) return false;
        current.current = value;
        const accepted = value.accountName.trim().length > 0 || value.items.some(item => item.status === 'checking');
        nativeAccepted.current ||= accepted;
        if (mounted.current) {
            if (idleExpiry) { setConfirm(false); setAcknowledged(false); }
            if (!resetAfterWork.current && workRef.current === 'prepare' && value.phase === 'preparing') setPreparationStage(value.preparationStage);
            if (nativeAccepted.current) clearDraft(); // An early cancellation ID is not account/preflight acceptance.
            if (nativeAccepted.current && !resetAfterWork.current) setSnapshot(value);
        }
        if (cancelWanted.current) void sendCancel();
        return nativeAccepted.current;
    };
    const finishWork = async () => {
        activeProgressToken.current = null;
        if (resetAfterWork.current && current.current) {
            const id = current.current.batchId;
            try {
                await discard(id);
                if (current.current?.batchId === id) current.current = null;
            } catch { if (mounted.current) setError('operation'); }
        }
        resetAfterWork.current = false;
        cancelWanted.current = false;
        cancelSent.current = null;
        workRef.current = null;
        lock.current = false;
        if (mounted.current) { setWork(null); setStopping(false); setPreparationStage(undefined); }
    };
    const notifyActivation = () => {
        if (!mounted.current || resetAfterWork.current) return;
        const value = current.current;
        if (value && value.items.some(item => item.status === 'success') && activatedBatch.current !== value.batchId) {
            activatedBatch.current = value.batchId;
            onActivated();
        }
    };

    const prepare = async (event: FormEvent) => {
        event.preventDefault();
        if (lock.current || !active) return;
        importSequence.current += 1;
        const parsed = parseRedemptionBatch(draft);
        if (!parsed.ok) { setIssues(parsed.issues); return; }
        lock.current = true;
        workRef.current = 'prepare';
        setWork('prepare'); setError(null); setIssues([]); setOfficial(null);
        setStoppedBeforeAcceptance(false);
        setPreparationStage(undefined);
        setLastOperation('check');
        nativeAccepted.current = false;
        expectedMasks.current = parsed.keys.map(row => row.masked);
        cancelWanted.current = false; cancelSent.current = null; resetAfterWork.current = false;
        const token = ++sequence.current;
        activeProgressToken.current = token;
        try {
            if (current.current) { await discard(current.current.batchId); current.current = null; }
            const onProgress = new Channel<RedemptionSnapshot>(value => { if (accept(value, token, undefined, true)) for (const row of parsed.keys) row.key = ''; });
            const value = await invoke<RedemptionSnapshot>('epic_redeem_batch_prepare', { keys: parsed.keys.map(row => row.key), onProgress });
            const accepted = accept(value, token);
            if (mounted.current && !resetAfterWork.current) {
                if (accepted) heading.current?.focus();
                else if (value.phase === 'stopped') setStoppedBeforeAcceptance(true);
                else setError('operation');
            }
        } catch (failure) { if (mounted.current && !resetAfterWork.current) setError(classifyError(failure)); }
        finally { for (const row of parsed.keys) row.key = ''; await finishWork(); }
    };
    const activate = async () => {
        const value = current.current;
        if (lock.current || !active || !acknowledged || error || !value || !value.accountName.trim() || value.phase !== 'ready' || !value.items.some(item => item.status === 'ready')) return;
        const eligible: RedemptionSnapshot = {
            ...value, phase: 'activating', expiresAt: undefined, preparationStage: undefined, waitSeconds: 0, waitReason: undefined,
            items: value.items.filter(item => item.status === 'ready').map((item, index) => ({ ...item, index })),
        };
        expectedMasks.current = eligible.items.map(item => item.maskedKey);
        current.current = eligible;
        setSnapshot(eligible);
        lock.current = true;
        workRef.current = 'activate'; setWork('activate'); setConfirm(false); setAcknowledged(false); setError(null);
        setPreparationStage(undefined);
        setLastOperation('activate');
        cancelWanted.current = false; cancelSent.current = null; resetAfterWork.current = false;
        const token = ++sequence.current;
        activeProgressToken.current = token;
        try {
            const onProgress = new Channel<RedemptionSnapshot>(next => { accept(next, token, value.batchId, true); });
            accept(await invoke<RedemptionSnapshot>('epic_redeem_batch_activate', { batchId: value.batchId, onProgress }), token, value.batchId);
        } catch (failure) { if (mounted.current && !resetAfterWork.current) setError(classifyError(failure)); }
        finally { notifyActivation(); await finishWork(); }
    };
    const stop = () => {
        if (!lock.current || workRef.current === 'official' || stopping) return;
        cancelWanted.current = true;
        setStopping(true);
        void sendCancel();
    };
    const startNew = async () => {
        if (lock.current) return;
        lock.current = true; workRef.current = 'discard'; setWork('discard'); setError(null);
        setPreparationStage(undefined);
        try {
            if (current.current) await discard(current.current.batchId);
            current.current = null; sequence.current += 1; expectedMasks.current = []; nativeAccepted.current = false; clearDraft(); setSnapshot(null); setOfficial(null); setLastOperation('check'); setStoppedBeforeAcceptance(false);
        } catch (failure) { setError(classifyError(failure)); }
        finally { await finishWork(); }
    };
    const importFile = async (event: ChangeEvent<HTMLInputElement>) => {
        const file = event.currentTarget.files?.[0];
        const token = ++importSequence.current;
        event.currentTarget.value = '';
        if (!file || lock.current || !active) return;
        if (file.size > REDEMPTION_BATCH_LIMITS.maxUtf8Bytes) { setIssues([{ code: 'inputTooLarge', limit: REDEMPTION_BATCH_LIMITS.maxUtf8Bytes }]); return; }
        try {
            const text = await file.text();
            if (!mounted.current || importSequence.current !== token) return;
            setDraft(text); setIssues([]); setError(null); setStoppedBeforeAcceptance(false); field.current?.focus();
        } catch { if (mounted.current && importSequence.current === token) setError('file'); }
    };
    const openOfficial = async () => {
        if (lock.current || !active) return;
        lock.current = true; workRef.current = 'official'; setWork('official'); setError(null); setOfficial(null);
        setPreparationStage(undefined);
        cancelWanted.current = false; cancelSent.current = null; resetAfterWork.current = false;
        let successful = false;
        const token = ++sequence.current;
        activeProgressToken.current = token;
        try {
            if (current.current) await discard(current.current.batchId);
            current.current = null; expectedMasks.current = []; nativeAccepted.current = false; setSnapshot(null); setStoppedBeforeAcceptance(false); clearDraft();
            const onResult = new Channel<OfficialResult>(value => {
                if (activeProgressToken.current !== token || sequence.current !== token) return;
                if (mounted.current && !resetAfterWork.current) setOfficial(value);
                if (mounted.current && !resetAfterWork.current && value.status === 'success' && !successful) { successful = true; onActivated(); }
            });
            await invoke('epic_open_redeem', { onResult });
            if (mounted.current && !resetAfterWork.current) setOfficial(value => value?.status === 'success' || value?.status === 'failed' ? value : { status: 'unknown', title: value?.title || '', errorCode: '' });
        } catch (failure) { if (mounted.current && !resetAfterWork.current) setError(classifyError(failure)); }
        finally { await finishWork(); }
    };

    const items = snapshot?.items ?? [];
    const preparing = lastOperation === 'check';
    const checked = items.filter(item => ['ready', 'success', 'failed', 'unknown', 'requiresOfficial'].includes(item.status)).length;
    const processed = items.filter(item => ['success', 'failed', 'unknown', 'requiresOfficial'].includes(item.status)).length;
    const successCount = items.filter(item => item.status === 'success').length;
    const readyCount = items.filter(item => item.status === 'ready').length;
    const quotaWait = snapshot?.waitReason === 'rateLimited' && (snapshot.waitSeconds ?? 0) > 0;
    const activatingQuotaWait = quotaWait && (snapshot?.phase === 'activating' || work === 'activate');
    const partialPreview = snapshot?.phase === 'ready' && readyCount > 0 && items.some(item => item.status === 'notAttempted');
    const unknown = items.some(item => item.status === 'unknown');
    const authRequired = error === 'auth' || items.some(item => item.reason === 'authRequired');
    const phaseLabel = error && !busy ? 'Операция не завершена.' : stopping ? 'Останавливаем после текущего ключа…' : work === 'official' ? 'Окно активации открыто'
        : work === 'prepare' && preparationStage ? preparationLabels[preparationStage]
        : work === 'prepare' && !snapshot ? 'Подготавливаем проверку ключей…'
        : snapshot?.phase === 'preparing' ? 'Проверяем ключи…'
        : snapshot?.phase === 'activating' || work === 'activate' ? 'Активируем ключи…'
        : snapshot?.phase === 'stopped' ? 'Обработка остановлена' : snapshot?.phase === 'completed' ? 'Обработка завершена' : 'Проверьте продукты перед активацией';

    return <div className="content"><section className="redeem-panel native-redemption">
        <h2>{tr('Активировать ключи')}</h2>
        <p>{tr('Сначала проверьте продукты и аккаунт. Активация начнётся только после отдельного подтверждения.')}</p>
        <ol className="redemption-steps" aria-label={tr('Этапы активации')}>
            {['Ввод ключей', 'Проверка продуктов', 'Результаты'].map((label, index) => <li key={label} aria-current={(!snapshot ? 0 : snapshot.phase === 'completed' || snapshot.phase === 'stopped' ? 2 : 1) === index ? 'step' : undefined}><span aria-hidden="true">{index + 1}</span>{tr(label)}</li>)}
        </ol>
        {!snapshot && <form onSubmit={event => void prepare(event)} className="redemption-input-form">
            <label htmlFor="redemption-keys">{tr('Ключи продуктов')}</label>
            <textarea ref={field} id="redemption-keys" value={draft} onChange={event => { importSequence.current += 1; setDraft(event.target.value); setIssues([]); setError(null); setStoppedBeforeAcceptance(false); }} disabled={busy} rows={4} maxLength={REDEMPTION_BATCH_LIMITS.maxUtf8Bytes} autoComplete="off" autoCapitalize="off" spellCheck={false} aria-describedby="redemption-input-help" aria-invalid={issues.length > 0}/>
            <p id="redemption-input-help" className="muted">{tr('Один ключ в строке. До 100 уникальных ключей; TXT до 64 КиБ. Пустые строки и повторы пропускаются.')}</p>
            <p className="muted">{tr('Ключи используются только для этой операции и не сохраняются в настройках или истории.')}</p>
            <input ref={fileInput} className="redemption-file-input" type="file" accept=".txt,text/plain" onChange={event => void importFile(event)} disabled={busy} tabIndex={-1} aria-label={tr('Импортировать TXT')}/>
            <div className="redemption-actions"><button type="button" disabled={busy} onClick={() => fileInput.current?.click()}><FileUp size={17}/>{tr('Импортировать TXT')}</button><button className="primary" type="submit" disabled={busy || !draft.trim()}><KeyRound size={17}/>{tr('Далее — проверить продукты')}</button></div>
        </form>}
        {!!issues.length && <ul className="redemption-errors error-text" role="alert">{issues.map((issue, index) => <li key={index}>{tr(issueLabel(issue))}</li>)}</ul>}
        {stoppedBeforeAcceptance && <p role="status">{tr('Обработка остановлена')}</p>}
        {(snapshot || busy && work !== 'official') && <div className="redemption-batch">
            <div className="redemption-batch-heading"><h3 ref={heading} tabIndex={-1}>{tr(phaseLabel)}</h3>{busy && work !== 'discard' && work !== 'official' && <button type="button" disabled={stopping} onClick={stop}><Square size={15}/>{tr('Остановить после текущего')}</button>}</div>
            <p className="redemption-wait redemption-wait-slot" role={snapshot?.waitSeconds ? 'status' : undefined} aria-live={snapshot?.waitSeconds ? 'polite' : undefined} aria-hidden={snapshot?.waitSeconds ? undefined : true}>{snapshot?.waitSeconds ? tr(snapshot.waitReason === 'batch' ? `Следующий пакет через ${number(snapshot.waitSeconds)} с` : snapshot.waitReason === 'beforeActivation' ? `Активация начнётся через ${number(snapshot.waitSeconds)} с` : activatingQuotaWait ? `Активация продолжится через ${number(snapshot.waitSeconds)} с` : quotaWait ? `Проверка продолжится через ${number(snapshot.waitSeconds)} с` : `Следующий ключ через ${number(snapshot.waitSeconds)} с`) : null}</p>
            {snapshot && <p className="redemption-account">{tr('Аккаунт Epic')}: <strong>{snapshot.accountName}</strong></p>}
            {snapshot && <div className="redemption-counts" role="status" aria-live="polite"><span>{tr(preparing ? 'Проверено' : 'Обработано')}: <b>{number(preparing ? checked : processed)}</b> / {number(items.length)}</span><span>{tr('Успешных активаций')}: <b>{number(successCount)}</b></span></div>}
            {items.length >= 10 && <p className="muted redemption-batch-note">{tr('Обрабатываем по 10 ключей. После паузы продолжим автоматически.')}</p>}
            {snapshot && <div className="redemption-table-scroll" tabIndex={0} role="region" aria-label={tr('Результаты проверки ключей')}><table className="redemption-table"><thead><tr><th scope="col">{tr('Ключ')}</th><th scope="col">{tr('Продукт')}</th><th scope="col">{tr('Статус')}</th></tr></thead><tbody>{items.map(item => <tr key={item.id} className={`redemption-row redemption-row-${item.status}`} aria-current={item.status === 'checking' || item.status === 'activating' ? 'true' : undefined}><td><code>{item.maskedKey}</code></td><td>{item.title || '—'}</td><td><strong>{tr(activatingQuotaWait && item.status === 'activating' && item.reason === 'rateLimited' ? 'Ожидаем продолжения активации' : quotaWait && item.status === 'checking' ? 'Ожидаем повторной проверки' : statusLabels[item.status])}</strong>{item.reason && <span>{tr(reasonLabels[item.reason] || reasonLabels.unknown)}</span>}<DiagnosticDetails value={item.diagnostic} maskedKey={item.maskedKey}/></td></tr>)}</tbody></table></div>}
            {partialPreview && <p className="redemption-warning" role="status">{tr('Проверена часть списка. Можно активировать готовые ключи; остальные не обработаны.')}</p>}
            {unknown && <p className="redemption-warning">{tr('Результат одной из активаций неизвестен. Автоматического повтора не будет. Проверьте результат у Epic.')}</p>}
            {snapshot && <div className="redemption-actions"><button type="button" disabled={busy} onClick={() => void startNew()}>{tr('Ввести другие ключи')}</button><button type="button" className="primary" disabled={busy || !!error || !snapshot.accountName.trim() || snapshot.phase !== 'ready' || readyCount === 0 || unknown} onClick={() => { setAcknowledged(false); setConfirm(true); }}>{tr('Активировать готовые ключи')} ({number(readyCount)})</button><button type="button" disabled={busy} onClick={onLibrary}><RefreshCw size={17}/>{tr('Открыть и обновить библиотеку')}</button></div>}
            {successCount > 0 && <p className="muted">{tr('Материалы Fab появятся после обновления библиотеки; игры в ней не отображаются.')}</p>}
        </div>}
        {authRequired && <EpicAuthError error="UM_EPIC_AUTH_REQUIRED"/>}
        {error && error !== 'auth' && <p className="error-text" role="alert">{tr(errorLabels[error])}</p>}
        <aside className="redemption-official"><h3>{tr('Официальное окно Epic')}</h3><p>{tr('Если ключ требует дополнительных действий, откройте официальный сайт Epic. Ключи из этого списка туда не передаются.')}</p><button type="button" disabled={busy} onClick={() => void openOfficial()}><ExternalLink size={17}/>{tr(busy && work === 'official' ? 'Окно активации открыто' : 'Открыть окно активации')}</button>{work === 'official' && <p role="status">{tr('Ожидаем активацию в окне Epic…')}</p>}
            {official && <div className={`redemption-result redemption-${official.status}`} role="status"><h3>{tr(official.status === 'success' ? 'Ключ успешно активирован' : official.status === 'failed' ? 'Активация не удалась' : official.status === 'ready' ? 'Ключ проверен — ожидаем подтверждения' : 'Результат активации не подтверждён')}</h3>{official.title && <p>{tr('Продукт:')} <strong>{official.title}</strong></p>}{official.status === 'ready' && <p>{tr('Ключ ещё не активирован. Завершите подтверждение в окне Epic.')}</p>}{official.status === 'unknown' && <p>{tr('Окно закрыто, связь прервана или ответ Epic не удалось распознать. Проверьте результат у Epic и обновите библиотеку.')}</p>}{official.status === 'failed' && <p>{tr(official.errorCode.endsWith('.code_used') ? 'Этот ключ уже использован.' : official.errorCode.includes('already') ? 'Этот ключ уже использован или продукт уже есть в аккаунте.' : official.errorCode.includes('invalid') ? 'Epic отклонил ключ. Проверьте введённые символы.' : 'Epic не подтвердил активацию. Точная причина показана в окне Epic.')}</p>}<button type="button" disabled={busy} onClick={onLibrary}>{tr('Открыть и обновить библиотеку')}</button></div>}
        </aside>
        {confirm && snapshot && createPortal(<ModalFocusScope onClose={() => { setConfirm(false); setAcknowledged(false); }} className="modal-backdrop redemption-confirm-backdrop"><section className="login-dialog redemption-confirm" role="dialog" aria-modal="true" aria-labelledby="redemption-confirm-title"><div className="modal-head"><h2 id="redemption-confirm-title">{tr('Подтвердите активацию')}</h2><button type="button" className="icon" aria-label={tr('Закрыть')} onClick={() => { setConfirm(false); setAcknowledged(false); }}><X size={20}/></button></div><div className="redemption-confirm-body"><p>{tr('Аккаунт Epic')}: <strong>{snapshot.accountName}</strong></p><p>{tr('Готово к активации')}: <strong>{number(readyCount)}</strong></p><p>{tr('Будут активированы только готовые ключи. Действие нельзя отменить. Остановка не отменяет уже выполненную активацию.')}</p><label className="redemption-confirm-check"><input data-modal-initial-focus type="checkbox" checked={acknowledged} onChange={event => setAcknowledged(event.target.checked)}/><span>{tr('Подтверждаю аккаунт и выбранные продукты.')}</span></label></div><div className="dialog-actions"><button type="button" onClick={() => { setConfirm(false); setAcknowledged(false); }}>{tr('Назад')}</button><button type="button" className="primary" disabled={!acknowledged || busy} onClick={() => void activate()}><CheckCircle2 size={17}/>{tr('Активировать готовые ключи')} ({number(readyCount)})</button></div></section></ModalFocusScope>, document.body)}
    </section></div>;
}
