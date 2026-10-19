import { tr } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { RefreshCw, FileX2 } from 'lucide-react';
export function EngineDelete({ path, onClose, onDeleted }: {
    path: string;
    onClose: () => void;
    onDeleted: () => Promise<void>;
}) {
    const [preview, setPreview] = useState<{path: string; version: string} | null>(null);
    const [error, setError] = useState(''), [busy, setBusy] = useState(false), [result, setResult] = useState('');
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
        try {
            const message = await invoke<string>('engine_delete', {path: preview.path, expectedVersion: preview.version});
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
    }}><section className="engine-wizard" role="alertdialog" aria-modal="true" aria-label={tr("Удаление движка")}>
        <h2>{tr("Удаление Unreal Engine ")}{preview?.version}</h2><p className="project-delete-path">{path}</p>
        {!result && <><p>{tr("Будет удалена вся папка движка, включая установленные в неё плагины. Проекты вне этой папки и Vault сохранятся.")}</p><p className="error-text">{tr("Файлы будут стёрты без возможности восстановления из корзины.")}</p></>}
        {!preview && !error && <p role="status">{tr("Проверяем установку…")}</p>}
        {busy && <p role="status"><RefreshCw size={17} className="spin"/> {tr("Удаляем…")}</p>}
        {error && <p role="alert" className="error-text">{tr(error)}</p>}
        {result && <p role="status">{tr(result)}</p>}
        <div className="wizard-footer">
            <button ref={cancel} disabled={busy} onClick={onClose}>{tr(result ? 'Закрыть' : 'Отмена')}</button>
            {!result && <button className="danger-button" disabled={!preview || busy} onClick={() => void remove()}><FileX2 size={17}/>{tr(busy ? 'Удаляем…' : 'Удалить навсегда')}</button>}
        </div>
    </section></div>, document.body);
}
