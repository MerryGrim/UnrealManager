import { Channel, invoke } from '@tauri-apps/api/core';
import { Download, RefreshCw, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { getLocale, tr } from './i18n';

type UpdateSettings = {
  enabled: boolean;
  channel: 'stable' | 'beta';
  configured: boolean;
  currentVersion: string;
  mode: 'installed' | 'portable';
  supported: boolean;
  lastApplyError?: string | null;
};

type Release = { version: string; notes: string; url: string; size: number; staged: boolean };
type CheckResult = {
  status: 'available' | 'current' | 'disabled' | 'deferred';
  release: Release | null;
  checkedAt: number;
  cached: boolean;
};
type Progress = { phase: string; received: number; total: number; percent: number };
type UpdateError = { title: string; detail: string };
type Phase = 'loading' | 'idle' | 'checking' | 'saving' | 'downloading' | 'applying';
type Startup = { settings: UpdateSettings; result: CheckResult | null; error: UpdateError | null };

// A shared promise also covers React StrictMode's development remount. There is
// one startup check per app process; native code enforces the daily network limit.
let startup: Promise<Startup> | null = null;
function startUpdates() {
  if (!startup) startup = (async () => {
    const settings = await invoke<UpdateSettings>('app_update_settings_get');
    if (!settings || typeof settings.enabled !== 'boolean') throw new Error('Invalid update settings');
    const previousError = settings.lastApplyError?.trim() ? { title: 'Предыдущее обновление не завершилось.', detail: settings.lastApplyError } : null;
    if (!settings.configured || !settings.supported) return { settings, result: null, error: previousError };
    try {
      return { settings, result: await invoke<CheckResult>('app_update_check', { manual: false }), error: previousError };
    } catch (error) {
      return { settings, result: null, error: previousError ?? { title: 'Не удалось проверить обновления.', detail: String(error) } };
    }
  })();
  return startup;
}

export function useAppUpdates() {
  const [settings, setSettings] = useState<UpdateSettings | null>(null);
  const [result, setResult] = useState<CheckResult | null>(null);
  const [phase, setPhase] = useState<Phase>('loading');
  const [error, setError] = useState<UpdateError | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [dismissedVersion, setDismissedVersion] = useState('');
  const lock = useRef(false);

  useEffect(() => {
    let active = true;
    void startUpdates().then(value => {
      if (!active) return;
      setSettings(value.settings);
      setResult(value.result);
      setError(value.error);
    }).catch(reason => {
      if (active) setError({ title: 'Не удалось загрузить настройки обновлений.', detail: String(reason) });
    }).finally(() => { if (active) setPhase('idle'); });
    return () => { active = false; };
  }, []);

  async function check() {
    if (lock.current || phase === 'loading') return;
    lock.current = true;
    setPhase('checking');
    setError(null);
    try {
      // A failed initial settings read can be retried with the manual action.
      const current = settings ?? await invoke<UpdateSettings>('app_update_settings_get');
      if (!current || typeof current.enabled !== 'boolean') throw new Error('Invalid update settings');
      setSettings(current);
      if (!current.configured || !current.supported) return;
      setResult(await invoke<CheckResult>('app_update_check', { manual: true }));
      setDismissedVersion('');
      setProgress(null);
    } catch (reason) {
      setError({ title: 'Не удалось проверить обновления.', detail: String(reason) });
    } finally {
      lock.current = false;
      setPhase('idle');
    }
  }

  async function save(enabled: boolean, channel: UpdateSettings['channel']) {
    if (!settings || lock.current) return;
    lock.current = true;
    setPhase('saving');
    setError(null);
    try {
      await invoke('app_update_settings_set', { enabled, channel });
      if (channel !== settings.channel) {
        setResult(null);
        setProgress(null);
        setDismissedVersion('');
      }
      setSettings({ ...settings, enabled, channel });
    } catch (reason) {
      setError({ title: 'Не удалось сохранить настройки обновлений.', detail: String(reason) });
    } finally {
      lock.current = false;
      setPhase('idle');
    }
  }

  async function download() {
    const release = result?.release;
    if (!release || !settings?.configured || !settings.supported || lock.current) return;
    lock.current = true;
    setPhase('downloading');
    setError(null);
    setProgress(null);
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = value => {
      const percent = Math.max(0, Math.min(100, Math.floor(value.percent || 0)));
      setProgress(previous => previous?.phase === value.phase && previous.percent === percent ? previous : { ...value, percent });
    };
    try {
      const verified = await invoke<Release>('app_update_download', { version: release.version, onProgress });
      if (verified.version !== release.version || !verified.staged) throw new Error('Update package was not staged');
      setResult(previous => previous ? { ...previous, release: verified } : previous);
    } catch (reason) {
      setError({ title: 'Не удалось скачать обновление.', detail: String(reason) });
    } finally {
      lock.current = false;
      setPhase('idle');
    }
  }

  async function apply() {
    const release = result?.release;
    if (!release?.staged || !settings?.configured || !settings.supported || lock.current) return;
    lock.current = true;
    setPhase('applying');
    setError(null);
    try {
      await invoke('app_update_apply', { version: release.version });
      // The native helper owns closing, replacement and restart. Keep the UI
      // locked after hand-off, instead of allowing a second apply request.
    } catch (reason) {
      lock.current = false;
      setPhase('idle');
      setError({ title: 'Не удалось применить обновление.', detail: String(reason) });
    }
  }

  const release = result?.release ?? null;
  return {
    settings, result, phase, error, progress, release,
    busy: phase !== 'idle',
    showNotice: !!(settings?.enabled && settings.configured && settings.supported && release && release.version !== dismissedVersion),
    check, save, download, apply,
    dismiss: () => setDismissedVersion(release?.version ?? ''),
  };
}

type Controller = ReturnType<typeof useAppUpdates>;

export function AppUpdateNotice({ updates, onOpen }: { updates: Controller; onOpen: () => void }) {
  if (!updates.showNotice) return null;
  return <div className="app-update-notice" role="status">
    <span><b>{tr('Доступно обновление Unreal Manager')}</b> <span>{updates.release?.version}</span></span>
    <div><button onClick={onOpen}>{tr('Открыть обновление')}</button><button className="icon" onClick={updates.dismiss} aria-label={tr('Напомнить при следующем запуске')} title={tr('Напомнить при следующем запуске')}><X size={17}/></button></div>
  </div>;
}

export function AppUpdateSettings({ updates }: { updates: Controller }) {
  const { settings, result, release, phase, busy, progress, error } = updates;
  const [confirm, setConfirm] = useState(false);
  const applyButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const supported = !!(settings?.configured && settings.supported);
  const percent = progress?.percent ?? 0;
  const verifying = !!progress && /verif|check|провер/i.test(progress.phase);

  useEffect(() => { if (confirm) cancelButton.current?.focus(); }, [confirm]);
  function closeConfirmation() {
    if (phase === 'applying') return;
    setConfirm(false);
    applyButton.current?.focus();
  }

  let status = 'Проверка ещё не выполнялась.';
  if (phase === 'loading') status = 'Настройки обновлений загружаются…';
  else if (phase === 'checking') status = 'Проверяем обновления…';
  else if (phase === 'downloading') status = verifying ? 'Проверяем пакет обновления…' : 'Загрузка обновления…';
  else if (phase === 'applying') status = 'Подготавливаем перезапуск…';
  else if (settings && !supported) status = 'Обновления недоступны для этой сборки.';
  else if (release?.staged) status = 'Пакет проверен и готов к установке.';
  else if (release) status = 'Доступна версия';
  else if (result?.status === 'current') status = 'Установлена последняя доступная версия.';
  else if (result?.status === 'disabled' || settings?.enabled === false) status = 'Автоматическая проверка отключена.';
  else if (result?.status === 'deferred') status = 'Автоматическая проверка отложена. Можно проверить вручную.';

  return <section className="app-update-settings" id="app-update-settings" aria-labelledby="app-update-heading">
    <h3 id="app-update-heading">{tr('Обновления Unreal Manager')}</h3>
    <p>{tr('Обновление самого приложения. Движки и материалы обновляются отдельно.')}</p>
    {settings && <p className="app-update-current">{tr('Текущая версия')}: <strong>{settings.currentVersion}</strong></p>}
    <div className="setting-row"><div><label htmlFor="app-updates-enabled"><b>{tr('Автоматически проверять обновления')}</b></label><span id="app-updates-frequency">{tr('При запуске, не чаще одного раза в сутки. Установка начинается только по вашему действию.')}</span></div><input id="app-updates-enabled" type="checkbox" aria-describedby="app-updates-frequency" disabled={busy || !settings || !supported} checked={settings?.enabled ?? false} onChange={event => { if (settings) void updates.save(event.target.checked, settings.channel); }}/></div>
    <div className="setting-row"><label htmlFor="app-update-channel"><b>{tr('Канал обновлений')}</b></label><select id="app-update-channel" disabled={busy || !settings || !supported} value={settings?.channel ?? 'stable'} onChange={event => { if (settings) void updates.save(settings.enabled, event.target.value as UpdateSettings['channel']); }}><option value="stable">{tr('Стабильные версии')}</option><option value="beta">{tr('Стабильные и бета-версии')}</option></select></div>
    <div className="app-update-state" role="status" aria-live="polite"><p>{tr(status)}{status === 'Доступна версия' && release && <> <strong>{release.version}</strong></>}</p>{phase === 'downloading' && <div className="app-update-progress"><progress aria-label={tr(verifying ? 'Проверяем пакет обновления…' : 'Загрузка обновления…')} max={100} value={percent}/><span>{percent}%</span></div>}{result?.checkedAt ? <small>{tr('Последняя проверка')}: {new Date(result.checkedAt * 1000).toLocaleString(getLocale())}</small> : null}</div>
    {release && <div className="app-update-release"><p><strong>{release.version}</strong>{release.staged && <span className="status-ok">{tr('Проверенный пакет')}</span>}</p>{release.notes && <details><summary>{tr('Изменения выпуска')}</summary><pre>{release.notes}</pre></details>}</div>}
    {error && <div className="error-text" role="alert"><p>{tr(error.title)}</p><details><summary>{tr('Подробности ошибки')}</summary><pre>{tr(error.detail)}</pre></details></div>}
    <div className="app-update-actions"><button disabled={busy || (!!settings && !supported)} onClick={() => void updates.check()}><RefreshCw size={16}/>{tr('Проверить обновления')}</button>{release && supported && (release.staged ? <button ref={applyButton} className="primary" disabled={busy} onClick={() => setConfirm(true)}>{tr('Установить и перезапустить')}</button> : <button className="primary" disabled={busy} onClick={() => void updates.download()}><Download size={16}/>{tr('Скачать обновление')}</button>)}</div>
    {confirm && createPortal(<div className="modal-backdrop app-update-backdrop" onKeyDown={event => {
      if (event.key === 'Escape') closeConfirmation();
      if (event.key === 'Tab') {
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        event.preventDefault();
        buttons[(index + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
      }
    }}><section className="app-update-confirm" role="dialog" aria-modal="true" aria-labelledby="app-update-confirm-title" aria-describedby="app-update-confirm-description"><h2 id="app-update-confirm-title">{tr('Обновить Unreal Manager?')}</h2><p id="app-update-confirm-description">{tr('Перед обновлением активные операции будут безопасно остановлены и сохранены. Приложение закроется, обновится и запустится снова. Настройки, сеанс Epic, Vault и проекты сохранятся. Продолжение задач запускается вручную.')}</p>{phase === 'applying' && <p role="status">{tr('Подготавливаем перезапуск…')}</p>}{error && <p className="error-text" role="alert">{tr(error.title)}</p>}<div className="app-update-actions"><button ref={cancelButton} disabled={phase === 'applying'} onClick={closeConfirmation}>{tr('Отмена')}</button><button className="primary" disabled={phase === 'applying'} onClick={() => void updates.apply()}>{tr('Установить и перезапустить')}</button></div></section></div>, document.body)}
  </section>;
}
