import { FolderClosed, FolderPlus, Trash2, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { ModalFocusScope } from './ModalFocusScope';
import { tr } from './i18n';

export function ProjectScanFoldersDialog({ onClose, onSaved }: { onClose: () => void; onSaved: () => void }) {
    const [folders, setFolders] = useState<string[]>([]);
    const [loading, setLoading] = useState(true);
    const [loaded, setLoaded] = useState(false);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const mounted = useRef(true);
    const lock = useRef(false);
    useEffect(() => {
        mounted.current = true;
        void invoke<string[]>('list_scan_folders').then(value => {
            if (mounted.current) { setFolders(value); setLoaded(true); }
        }).catch(failure => {
            if (mounted.current) setError(String(failure));
        }).finally(() => {
            if (mounted.current) setLoading(false);
        });
        return () => { mounted.current = false; };
    }, []);

    const add = async () => {
        if (!loaded || loading || lock.current || folders.length >= 16) return;
        lock.current = true;
        setBusy(true);
        setError('');
        try {
            const selected = await invoke<string | null>('choose_project_location', { folder: true });
            if (!selected || !mounted.current) return;
            const comparable = (value: string) => value.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
            setFolders(current => current.some(value => comparable(value) === comparable(selected)) ? current : [...current, selected]);
        } catch (failure) {
            if (mounted.current) setError(String(failure));
        } finally {
            lock.current = false;
            if (mounted.current) setBusy(false);
        }
    };
    const save = async () => {
        if (!loaded || loading || lock.current) return;
        lock.current = true;
        setBusy(true);
        setError('');
        try {
            await invoke<string[]>('set_scan_folders', { folders });
            if (mounted.current) onSaved();
        } catch (failure) {
            if (mounted.current) setError(String(failure));
        } finally {
            lock.current = false;
            if (mounted.current) setBusy(false);
        }
    };
    return createPortal(<ModalFocusScope onClose={onClose} closeDisabled={busy}>
        <section className="project-scan-folders-dialog" role="dialog" aria-modal="true" aria-labelledby="project-scan-folders-title">
            <div className="modal-head"><h2 id="project-scan-folders-title">{tr('Папки проектов')}</h2><button className="icon" data-modal-initial-focus aria-label={tr('Закрыть')} disabled={busy} onClick={onClose}><X size={20}/></button></div>
            <p className="muted">{tr('Кнопка «Обновить список» ищет проекты в этих папках и их подпапках.')}</p>
            {loading ? <p role="status">{tr('Загружаем папки…')}</p> : <>
                <ul className="project-scan-folder-list">{folders.map((folder, index) => <li key={folder}>
                    <FolderClosed size={18}/><span title={folder} translate="no">{folder}</span>
                    <button className="icon" aria-label={`${tr('Убрать папку из списка')}: ${folder}`} title={tr('Убрать папку из списка')} disabled={busy} onClick={() => setFolders(current => current.filter((_, position) => position !== index))}><Trash2 size={17}/></button>
                </li>)}</ul>
                {!folders.length && <p className="muted">{tr('Папки не выбраны.')}</p>}
                <button disabled={!loaded || busy || folders.length >= 16} onClick={() => void add()}><FolderPlus size={17}/>{tr('Добавить папку')}</button>
                {folders.length >= 16 && <p className="muted">{tr('Можно выбрать до 16 папок.')}</p>}
                <p className="project-scan-folder-note muted">{tr('Удаление папки из списка не удаляет проекты с диска.')}</p>
            </>}
            {error && <p className="error-text" role="alert">{tr(error)}</p>}
            <div className="wizard-footer"><button disabled={busy} onClick={onClose}>{tr('Отмена')}</button><button className="primary" disabled={!loaded || loading || busy} onClick={() => void save()}>{tr(busy ? 'Сохраняем…' : 'Сохранить')}</button></div>
        </section>
    </ModalFocusScope>, document.body);
}
