import {invoke} from '@tauri-apps/api/core';

let acknowledgement: Promise<boolean> | null = null;
// Called by App's post-commit effect. main.tsx mounts App only after local
// preferences/i18n initialization succeeded. Network checks and drive scans
// are optional workloads and must not delay this bounded startup handshake.
export function acknowledgeMountedApplication(): Promise<boolean> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return Promise.resolve(false);
  return acknowledgement ??= invoke<boolean>('app_update_ui_ready');
}
