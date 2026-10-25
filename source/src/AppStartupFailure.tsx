import { WorkspaceHeader } from './WorkspaceHeader';
import { ClosePrompt } from './WindowSettings';
import { tr, useLanguage } from './i18n';

// A settings failure must leave native window controls and the normal close flow usable.
// Keep the invalid file intact so Retry can read a repaired file without losing preferences.
export function AppStartupFailure({ error, onRetry }: { error: string; onRetry: () => void }) {
  useLanguage();
  return <div className="app-shell">
    <WorkspaceHeader section="home" items={[]} onNavigate={() => {}} accountMenu={null} />
    <main className="content startup-failure">
      <div className="empty" role="alert"><h1>{tr('Не удалось загрузить настройки')}</h1><p>{error}</p><button type="button" onClick={onRetry}>{tr('Повторить')}</button></div>
    </main>
    <ClosePrompt />
  </div>;
}
