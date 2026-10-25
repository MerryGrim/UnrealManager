import { Download, FolderKanban, HardDrive, Monitor, RefreshCw } from 'lucide-react';
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { AppUpdateSettings, type useAppUpdates } from './AppUpdateSettings';
import { CacheSettings } from './CacheSettings';
import { DownloadSettings } from './DownloadSettings';
import { LanguageSettings } from './LanguageSettings';
import { ProjectAssociationSettings } from './ProjectAssociationSettings';
import { VaultStorageSettings } from './VaultStorageSettings';
import { WindowSettings } from './WindowSettings';
import { ThemeSettings } from './ThemeSettings';
import { tr } from './i18n';

const categories = [
  { id: 'interface', label: 'Интерфейс', icon: Monitor },
  { id: 'downloads', label: 'Загрузки', icon: Download },
  { id: 'storage', label: 'Хранилище', icon: HardDrive },
  { id: 'projects', label: 'Проекты', icon: FolderKanban },
  { id: 'appupdates', label: 'Обновления приложения', icon: RefreshCw },
] as const;

export type SettingsCategory = typeof categories[number]['id'];

type Props = {
  category: SettingsCategory;
  onCategoryChange: (category: SettingsCategory) => void;
  appUpdates: ReturnType<typeof useAppUpdates>;
  vaultBusy: boolean;
  onVaultChanged: () => Promise<void>;
  epicVaultSettings: ReactNode;
};

export function SettingsView({ category, onCategoryChange, appUpdates, vaultBusy, onVaultChanged, epicVaultSettings }: Props) {
  const [visited, setVisited] = useState(() => new Set<SettingsCategory>([category]));
  const tabs = useRef<Partial<Record<SettingsCategory, HTMLButtonElement | null>>>({});

  // Keep a visited category mounted so local, unsaved form state survives a switch.
  useEffect(() => {
    setVisited(previous => previous.has(category) ? previous : new Set([...previous, category]));
  }, [category]);

  function select(next: SettingsCategory) {
    setVisited(previous => previous.has(next) ? previous : new Set([...previous, next]));
    onCategoryChange(next);
  }

  function navigate(event: KeyboardEvent<HTMLButtonElement>, current: SettingsCategory) {
    const index = categories.findIndex(item => item.id === current);
    let nextIndex: number;
    switch (event.key) {
      case 'ArrowDown': case 'ArrowRight': nextIndex = (index + 1) % categories.length; break;
      case 'ArrowUp': case 'ArrowLeft': nextIndex = (index + categories.length - 1) % categories.length; break;
      case 'Home': nextIndex = 0; break;
      case 'End': nextIndex = categories.length - 1; break;
      default: return;
    }
    event.preventDefault();
    const next = categories[nextIndex].id;
    select(next);
    tabs.current[next]?.focus();
  }

  function contents(id: SettingsCategory) {
    switch (id) {
      case 'interface': return <><ThemeSettings /><LanguageSettings /><WindowSettings /><InterfaceGraphicsSettings /></>;
      case 'downloads': return <DownloadSettings />;
      case 'storage': return <><VaultStorageSettings busy={vaultBusy} onChanged={onVaultChanged} />{epicVaultSettings}<CacheSettings /></>;
      case 'projects': return <ProjectAssociationSettings />;
      case 'appupdates': return <AppUpdateSettings updates={appUpdates} />;
    }
  }

  return <div className="content settings settings-layout">
    <div className="settings-sidebar">
      <p className="settings-navigation-heading">{tr('Разделы настроек')}</p>
      <div className="settings-navigation" role="tablist" aria-label={tr('Разделы настроек')} aria-orientation="vertical">
        {categories.map(({ id, label, icon: Icon }) => <button
          key={id}
          ref={element => { tabs.current[id] = element; }}
          type="button"
          role="tab"
          id={`settings-tab-${id}`}
          aria-controls={`settings-panel-${id}`}
          aria-selected={category === id}
          tabIndex={category === id ? 0 : -1}
          data-settings-category={id}
          onClick={() => select(id)}
          onKeyDown={event => navigate(event, id)}
        ><Icon size={18} aria-hidden="true" /><span>{tr(label)}</span></button>)}
      </div>
    </div>
    <div className="settings-content">
      {categories.map(({ id, label }) => <div
        key={id}
        className="settings-panel"
        role="tabpanel"
        id={`settings-panel-${id}`}
        aria-labelledby={`settings-tab-${id}`}
        hidden={category !== id}
        tabIndex={0}
        data-settings-category={id}
      >{(visited.has(id) || category === id) && <><h2 className="settings-category-title">{tr(label)}</h2>{contents(id)}</>}</div>)}
    </div>
  </div>;
}

function InterfaceGraphicsSettings() {
  return <section>
    <span className="eyebrow">{tr('ПРОИЗВОДИТЕЛЬНОСТЬ')}</span>
    <h3>{tr('Графика интерфейса')}</h3>
    <div className="setting-row"><div><b>{tr('Программная отрисовка')}</b><span>{tr('Аппаратное ускорение WebView2 отключено. Состояние дискретной GPU проверяется внешним мониторингом.')}</span></div><span className="status-ok">{tr('ВКЛЮЧЕНО')}</span></div>
    <div className="setting-row"><div><b>{tr('Анимации интерфейса')}</b><span>{tr('Только короткие событийные transform/opacity переходы. В простое анимации отсутствуют.')}</span></div><span>{tr('Экономичные')}</span></div>
    <div className="setting-row"><div><b>{tr('Фоновая отрисовка')}</b><span>{tr('Скрытое окно не выполняет постоянные визуальные обновления.')}</span></div><span className="status-ok">{tr('Отключена')}</span></div>
  </section>;
}
