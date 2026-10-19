import { tr, getLocale } from './i18n';
import { useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { FolderOpen, RefreshCw, ShieldCheck, X } from 'lucide-react';
import type { VaultPackage } from './Vault';
import type { EngineInstall, UnrealProject } from './lib/backend';
import { engineVersion } from './lib/materialVersions';
type Preview = {
    target: string;
    token: string;
    compatible: boolean;
    changed: string[];
    added: string[];
    conflicts: string[];
    localChanges: string[];
    removed: string[];
    unchanged: number;
    alreadyCurrent: number;
    allFiles: number;
    backupName: string;
    backupDirectory: string;
};
type Result = {
    copied: number;
    preserved: number;
    backupPath: string | null;
};
export function ProjectUpdateDialog({ items, projects, engines, initial, initialNewId, busy, onBusy, onClose, onDone }: {
    items: VaultPackage[];
    projects: UnrealProject[];
    engines: EngineInstall[];
    initial?: VaultPackage;
    initialNewId?: string;
    busy: boolean;
    onBusy: (value: boolean) => void;
    onClose: () => void;
    onDone: () => Promise<void>;
}) {
    const [oldId, setOldId] = useState(initial?.id || ''), [newId, setNewId] = useState(initialNewId || ''), [target, setTarget] = useState(''), [destination, setDestination] = useState<'project' | 'engine'>('project');
    const [step, setStep] = useState(0), [backup, setBackup] = useState(true), [mode, setMode] = useState<'changed' | 'all'>('changed'), [accept, setAccept] = useState(false);
    const [preview, setPreview] = useState<Preview | null>(null), [result, setResult] = useState<Result | null>(null), [error, setError] = useState(''), [progress, setProgress] = useState('');
    const lock = useRef(false);
    const old = items.find(p => p.id === oldId), fresh = items.find(p => p.id === newId);
    const sameMaterial = (a: VaultPackage, b: VaultPackage) => !!a.assetId && a.assetId === b.assetId && a.kind === b.kind && a.id !== b.id && a.engines.some(e => b.engines.map(engineVersion).includes(engineVersion(e)));
    const candidates = old ? items.filter(p => sameMaterial(old, p)) : [];
    const sources = items.filter(p => items.some(other => sameMaterial(p, other)));
    const label = (p: VaultPackage) => `${p.title} · UE ${p.engines.map(engineVersion).join(', ')} · ${p.buildVersion || p.source || 'Vault'} · ${p.id.slice(0, 7)}`;
    const reset = () => { setPreview(null); setStep(0); setError(''); setAccept(false); };
    const inspect = async () => { if (lock.current || busy)
        return; lock.current = true; onBusy(true); setError(''); setProgress('Сравниваем пакеты и файлы назначения…'); try {
        setPreview(await invoke<Preview>('project_update_preview', { oldId, newId, target }));
        setStep(1);
    }
    catch (e) {
        setError(String(e));
    }
    finally {
        lock.current = false;
        onBusy(false);
        setProgress('');
    } };
    const browse = async () => { if (lock.current || busy)
        return; lock.current = true; onBusy(true); setError(''); try {
        const path = await invoke<string | null>('choose_project_location', { folder: false });
        if (path) {
            setTarget(path);
            reset();
        }
    }
    catch (e) {
        setError(String(e));
    }
    finally {
        lock.current = false;
        onBusy(false);
    } };
    const apply = async () => { if (!preview || lock.current || busy)
        return; lock.current = true; onBusy(true); setError(''); setProgress(backup ? 'Создаём резервную копию…' : 'Подготавливаем замену…'); try {
        const onProgress = new Channel<string>(setProgress);
        const next = await invoke<Result>('project_update_apply', { oldId, newId, target, token: preview.token, mode, backup, acceptVersion: accept, onProgress });
        setResult(next);
        await onDone();
    }
    catch (e) {
        setError(String(e));
    }
    finally {
        lock.current = false;
        onBusy(false);
        setProgress('');
    } };
    const count = preview ? (mode === 'all' ? preview.allFiles : preview.changed.length + preview.added.length) : 0;
    return <div className="modal-backdrop" onKeyDown={e => { if (e.key === 'Escape' && !busy)
        onClose(); }}><section className="login-dialog vault-dialog project-update-dialog" role="dialog" aria-modal="true" aria-label={tr("Обновление установленного материала")}>
  <div className="section-head"><h2>{tr(old?.kind === 'Plugin' ? 'Обновление плагина' : 'Обновление в проекте')}</h2><button disabled={busy} aria-label={tr("Закрыть обновление")} onClick={onClose}><X /></button></div>
  {tr(!result && <ol className="vault-steps"><li className={step === 0 ? 'current' : 'done'}>{tr("1. Материал и место")}</li><li className={step === 1 ? 'current' : step > 1 ? 'done' : ''}>{tr("2. Резервная копия")}</li><li className={step === 2 ? 'current' : ''}>{tr("3. Замена файлов")}</li></ol>)}
  {tr(!result && step === 0 && <div className="wizard-fields"><p className="muted">{tr("Выберите копию, из которой материал был установлен, и свежую копию из Vault. Обе нужны для точного сравнения изменений автора.")}</p>
   {tr(!sources.length && <p>{tr("Сначала скачайте свежую версию материала в Vault, сохранив предыдущую. Для старых пакетов без идентификатора Fab сравнение недоступно.")}</p>)}
   {tr(initial && initialNewId ? <div className="wizard-summary"><b>{initial.title}</b><p>{tr("Исходная и свежая копии выбраны по результату проверки обновлений.")}</p></div> : <><label className="wizard-field">{tr("Исходная версия")}<select disabled={busy} value={oldId} onChange={e => { setOldId(e.target.value); setNewId(''); setTarget(''); setDestination('project'); reset(); }}><option value="">{tr("Из какой копии устанавливали?")}</option>{tr(sources.map(p => <option key={p.id} value={p.id}>{tr(label(p))}</option>))}</select></label>
   <label className="wizard-field">{tr("Свежая версия")}<select disabled={busy || !old} value={newId} onChange={e => { setNewId(e.target.value); reset(); }}><option value="">{tr("Выберите скачанную копию")}</option>{tr(candidates.map(p => <option key={p.id} value={p.id}>{tr(label(p))}</option>))}</select></label></>)}
   {tr(old?.kind === 'Plugin' && <label className="wizard-field">{tr("Где обновлять")}<select disabled={busy} value={destination} onChange={e => { setDestination(e.target.value as 'project' | 'engine'); setTarget(''); reset(); }}><option value="project">{tr("В проекте")}</option><option value="engine">{tr("На движке")}</option></select></label>)}
   {tr(destination === 'engine' ? <label className="wizard-field">{tr("Установленный движок")}<select disabled={busy} value={target} onChange={e => { setTarget(e.target.value); reset(); }}><option value="">{tr("Выберите движок")}</option>{tr(engines.filter(e => fresh?.engines.map(engineVersion).includes(e.version)).map(e => <option key={e.path} value={e.path}>{tr("UE ")}{e.version}{tr(" · ")}{e.path}</option>))}</select></label> : <><label className="wizard-field">{tr("Проект")}<select disabled={busy} value={target} onChange={e => { setTarget(e.target.value); reset(); }}><option value="">{tr("Выберите проект")}</option>{tr(projects.map(p => <option key={p.path} value={p.path}>{p.name}{tr(" · ")}{tr(p.engineAssociation)}</option>))}</select></label><div className="path-picker"><input aria-label={tr("Проект для обновления")} disabled={busy} value={target} placeholder={tr("Полный путь к .uproject")} onChange={e => { setTarget(e.target.value); reset(); }}/><button disabled={busy} onClick={() => void browse()}><FolderOpen size={17}/>{tr("Выбрать…")}</button></div></>)}
   <div className="wizard-footer"><button disabled={busy} onClick={onClose}>{tr("Отказаться от обновления")}</button><button className="primary" disabled={busy || !old || !fresh || !target} onClick={() => void inspect()}>{tr("Далее")}</button></div>
  </div>)}
  {tr(!result && preview && step === 1 && <><h3><ShieldCheck size={19}/>{tr(" Резервная копия")}</h3><label className="downloaded-filter"><input type="checkbox" disabled={busy} checked={backup} onChange={e => setBackup(e.target.checked)}/>{tr("Сделать резервную копию перед обновлением")}</label><p>{tr("Копируем ")}{tr(old?.kind === 'Plugin' ? 'папку установленного плагина' : 'всю папку проекта')}{tr(" в отдельную папку с припиской к имени.")}</p><p className="project-delete-path">{preview.backupDirectory}{" / "}{tr(preview.backupName)}{tr("_…")}</p><p className="muted">{tr("Дата и время в названии; короткий уникальный суффикс защищает предыдущие копии. Если копия не создастся, замена не начнётся.")}</p><div className="wizard-footer"><button disabled={busy} onClick={() => setStep(0)}>{tr("Назад")}</button><button disabled={busy} onClick={onClose}>{tr("Отказаться")}</button><button className="primary" disabled={busy} onClick={() => setStep(2)}>{tr("Далее")}</button></div></>)}
  {tr(!result && preview && step === 2 && <><p className="project-delete-path">{tr(preview.target)}</p><fieldset className="update-modes" disabled={busy}><legend>{tr("Способ замены")}</legend><label><input type="radio" name="replace-mode" checked={mode === 'changed'} onChange={() => setMode('changed')}/><span><b>{tr("Обновить только изменённое")}</b><small>{tr("Копировать новые и изменённые автором файлы. Ваши правки и удалённые вами файлы сохранятся.")}</small></span></label><label><input type="radio" name="replace-mode" checked={mode === 'all'} onChange={() => setMode('all')}/><span><b>{tr("Заменить всё")}</b><small>{tr("Заново скопировать все файлы свежего пакета, включая совпадающие. Ваши изменения в этих файлах будут заменены.")}</small></span></label></fieldset>
   <div className="update-summary"><span>{tr("Изменено: ")}{tr(preview.changed.length)}</span><span>{tr("Новых: ")}{tr(preview.added.length)}</span><span>{tr("Не менялись: ")}{tr(preview.unchanged)}</span><span>{tr("Уже обновлены: ")}{tr(preview.alreadyCurrent)}</span></div>
   {tr([["Изменённые файлы", preview.changed], ["Новые файлы", preview.added], [mode === 'changed' ? "Ваши изменения — сохранятся" : "Ваши изменения — будут заменены", mode === 'changed' ? preview.conflicts : preview.localChanges], ["Удалены автором — оставим на месте", preview.removed]].map(([title, files]) => (files as string[]).length > 0 && <details className="update-file-list" key={title as string}><summary>{tr(title as string)}{tr(" (")}{tr((files as string[]).length)}{tr(")")}</summary><ul>{tr((files as string[]).map(file => <li key={file}>{tr(file)}</li>))}</ul></details>))}
   {tr(old?.kind === 'Project' && <p className="muted">{tr("Имя проекта, версия движка и настройки в .uproject сохранятся.")}</p>)}
   {tr(!preview.compatible && <label className="downloaded-filter"><input type="checkbox" checked={accept} disabled={busy} onChange={e => setAccept(e.target.checked)}/>{tr("Версия проекта отличается. Использовать выбранный пакет.")}</label>)}
   <p className="muted">{tr("Перед обновлением закройте Unreal Editor. ")}{tr(backup ? 'Сначала будет создана резервная копия.' : 'Резервная копия отключена.')}</p>
   <div className="wizard-footer"><button disabled={busy} onClick={() => setStep(1)}>{tr("Назад")}</button><button disabled={busy} onClick={onClose}>{tr("Отказаться от обновления")}</button><button className="primary" disabled={busy || !count || (!preview.compatible && !accept)} onClick={() => void apply()}><RefreshCw size={17}/>{tr("Обновить ")}{tr(count)}{tr(" файлов")}</button></div>
  </>)}
  {tr(progress && <p role="status" className="update-operation"><RefreshCw className="spin" size={18}/>{tr(progress)}{tr(" · Дождитесь завершения перед закрытием приложения.")}</p>)}{tr(error && <p role="alert" className="error-text">{tr(error)}</p>)}
  {tr(result && <><p className="vault-success" role="status">{tr("Обновлено файлов: ")}{tr(result.copied)}{tr(". Сохранено конфликтующих файлов: ")}{tr(result.preserved)}{tr(".")}</p>{tr(result.backupPath && <><p>{tr("Резервная копия:")}</p><p className="project-delete-path">{tr(result.backupPath)}</p></>)}<div className="wizard-footer"><button className="primary" onClick={onClose}>{tr("Готово")}</button></div></>)}
 </section></div>;
}
