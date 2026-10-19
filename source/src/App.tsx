import {ClosePrompt,WindowSettings} from './WindowSettings';
import {AppUpdateNotice,AppUpdateSettings,useAppUpdates} from './AppUpdateSettings';
import {DownloadSettings} from './DownloadSettings';
import type {EngineTransfer} from './EngineTransferCard';
import {CacheSettings} from './CacheSettings';
import {VaultStorageSettings} from './VaultStorageSettings';
import {ProjectAssociationSettings} from './ProjectAssociationSettings';
import { tr, getLocale } from './i18n';
import metadata from '../package.json';
import { AboutDialog } from './AboutDialog';
import { LanguageSettings } from './LanguageSettings';
import { useLanguage } from './i18n';
import { EnginesView } from './EnginesView';
import { UpdatesView } from './UpdatesView';
import { PageActions, PageActionsTarget } from './PageActions';
import logo from './assets/um-logo.svg';
import { AccountMenu } from './AccountMenu';
import { PluginsView } from './PluginsView';
import { ProjectTile } from './ProjectTile';
import { useVault } from './Vault';
import { useEffect, useMemo, useState } from 'react';
import { useEpic, FabLibrary, RedeemView } from './EpicLibrary';
import { Boxes, Download, FolderKanban, HardDrive, Home, Library, PackageOpen, PlugZap, RefreshCw, Rocket, Search, Settings, Wrench } from 'lucide-react';
import { scanEngines, scanProjects, type EngineInstall, type UnrealProject } from './lib/backend';
type Section = 'home' | 'engines' | 'projects' | 'library' | 'plugins' | 'updates' | 'downloads' | 'settings' | 'redeem';
const nav: {
    id: Section;
    label: string;
    icon: any;
    badge?: number;
}[] = [
    { id: 'home', label: 'Главная', icon: Home },
    { id: 'engines', label: 'Unreal Engine', icon: Boxes },
    { id: 'projects', label: 'Проекты', icon: FolderKanban },
    { id: 'library', label: 'Библиотека', icon: Library },
    { id: 'plugins', label: 'Плагины', icon: PlugZap },
    { id: 'updates', label: 'Обновления', icon: RefreshCw, badge: 0 },
    { id: 'downloads', label: 'Загрузки', icon: Download },
    { id: 'redeem', label: 'Активировать ключ', icon: PackageOpen },
    { id: 'settings', label: 'Настройки', icon: Settings },
];
export default function App() {
    useLanguage();
    const appUpdates = useAppUpdates();
    const [about, setAbout] = useState(false);
    const epic = useEpic();
    const [actionsTarget, setActionsTarget] = useState<HTMLDivElement | null>(null);
    const [section, setSection] = useState<Section>('home');
    useEffect(()=>{const vaultResume=()=>setSection('downloads');window.addEventListener('um-resume-vault',vaultResume);const resume=()=>setSection('engines');window.addEventListener('um-resume-engine',resume);return()=>{window.removeEventListener('um-resume-engine',resume);window.removeEventListener('um-resume-vault',vaultResume);};},[]);
    const [libraryRefresh, setLibraryRefresh] = useState(0);
    const [pluginLibraryRequest, setPluginLibraryRequest] = useState(0);
    const [updateCount, setUpdateCount] = useState(0);
    const [engineProgress, setEngineProgress] = useState<EngineTransfer | null>(null);
    const [engines, setEngines] = useState<EngineInstall[]>([]);
    const [projects, setProjects] = useState<UnrealProject[]>([]);
    const vault = useVault(projects, async () => { try {
        setProjects(await scanProjects());
    }
    catch (e) {
        setError(`Проект установлен, но список не удалось обновить: ${String(e)}`);
    } }, engines, () => setSection('plugins'), engineProgress ? {progress:engineProgress,onOpen:()=>setSection('engines')} : null);
    const [query, setQuery] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');
    const refresh = async () => {
        setLoading(true);
        setError('');
        try {
            const [e, p] = await Promise.all([scanEngines(), scanProjects()]);
            setEngines(e);
            setProjects(p);
        }
        catch (e) {
            setError(`Не удалось выполнить поиск: ${String(e)}`);
        }
        finally {
            setLoading(false);
        }
    };
    useEffect(() => { refresh(); }, []);
    const filteredProjects = useMemo(() => projects.filter(p => p.name.toLowerCase().includes(query.toLowerCase())), [projects, query]);
    return <div className="app-shell">
    <header className="workspace-topbar">
      <button className="workspace-brand" onClick={() => setSection('home')} aria-label={tr("Unreal Manager — Главная")}><img className="workspace-logo" src={logo} alt={tr("")}/><span>{tr("Unreal Manager")}<small>{metadata.version}</small></span></button>
      <nav className="workspace-nav" aria-label={tr("Основные разделы")}>{tr(nav.filter(item => ['engines', 'projects', 'library', 'plugins', 'updates'].includes(item.id)).map(item => { const I = item.icon; return <button key={item.id} onClick={() => setSection(item.id)} className={section === item.id ? 'active' : ''} aria-current={section === item.id ? 'page' : undefined}><I size={17}/><span>{tr(item.id === 'engines' ? 'Движки' : item.label)}</span>{tr(item.id === 'updates' && updateCount > 0 && <em className="nav-update-count">{tr(updateCount)}</em>)}</button>; }))}</nav>
      <AccountMenu account={epic.account} busy={epic.accountBusy} onLogin={epic.openLogin} onLogout={epic.logout} onRedeem={() => setSection('redeem')} onAbout={() => setAbout(true)}/>
    </header>
    <PageActionsTarget.Provider value={actionsTarget}><main className={`main workspace-main ${section === 'home' ? 'home-main' : ''}`}>
      {tr(section !== 'home' && <header className="workspace-heading"><h1>{tr(title(section))}</h1><div className="page-actions" ref={setActionsTarget}></div></header>)}
      <div className="workspace-page">      {tr(error ? <div role="alert" className="empty">{tr(error)}<button onClick={refresh}>{tr("Повторить")}</button></div> : section === 'home' && <HomeView downloaded={vault.items.length} engines={engines} projects={projects} onGo={setSection} onRemoved={path => setProjects(current => current.filter(p => p.path !== path))}/>)}      
      <PageActionsTarget.Provider value={section === 'engines' ? actionsTarget : null}><div className="library-view" hidden={section !== 'engines'}><EnginesView active={section === 'engines'} engines={engines} onRefresh={refresh} onProgress={setEngineProgress}/></div></PageActionsTarget.Provider>      
      {tr(section === 'projects' && <ProjectsView projects={filteredProjects} query={query} setQuery={setQuery} refresh={refresh} onRemoved={path => setProjects(current => current.filter(p => p.path !== path))}/>)}      
      <PageActionsTarget.Provider value={section === 'library' ? actionsTarget : null}><div className="library-view" hidden={section !== 'library'}><FabLibrary pluginRequest={pluginLibraryRequest} account={epic.account} onLogin={epic.openLogin} onDownload={vault.select} downloadBusy={vault.busy} downloadProgress={vault.tileProgress} pendingProgress={vault.pendingProgress} onResume={id=>window.dispatchEvent(new CustomEvent('um-resume-vault',{detail:id}))} refreshSignal={libraryRefresh} downloaded={vault.items} onVaultChanged={vault.refresh}/></div></PageActionsTarget.Provider>
      {tr(section === 'plugins' && <PluginsView onDownload={item => vault.select(item, false, true)} transfers={vault.updateTransfers} downloadProgress={vault.tileProgress} pendingProgress={vault.pendingProgress} onResume={id=>window.dispatchEvent(new CustomEvent('um-resume-vault',{detail:id}))} items={vault.items} busy={vault.busy} onInstall={vault.openInstall} onUpdate={vault.openProjectUpdate} onLibrary={() => { setPluginLibraryRequest(n => n + 1); setSection('library'); }} onRefresh={vault.refresh}/>)}
      <PageActionsTarget.Provider value={section === 'updates' ? actionsTarget : null}><div className="library-view" hidden={section !== 'updates'}><UpdatesView transfers={vault.updateTransfers} items={vault.items} account={epic.account} busy={vault.busy} onDownload={vault.openUpdate} onProjectUpdate={vault.openProjectUpdate} onCount={setUpdateCount}/></div></PageActionsTarget.Provider>
      {section === 'downloads' && vault.view}
      {tr(section === 'settings' && <SettingsView appUpdates={appUpdates} vaultSettings={<><ProjectAssociationSettings/><VaultStorageSettings busy={vault.busy||!!engineProgress} onChanged={vault.refresh}/>{vault.epicSettings}</>}/>)}
      <div className="library-view" hidden={section !== 'redeem'}><RedeemView account={epic.account} onActivated={() => setLibraryRefresh(n => n + 1)} onLibrary={() => { setSection('library'); setLibraryRefresh(n => n + 1); }}/></div>
      </div>
    </main></PageActionsTarget.Provider>
    <footer className="workspace-footer"><button className={section === 'downloads' ? 'active' : ''} aria-current={section === 'downloads' ? 'page' : undefined} onClick={() => setSection('downloads')}><Download size={16}/>{tr((engineProgress ? `${engineProgress.paused?tr('На паузе'):engineProgress.phase}${engineProgress.total ? ` · ${engineProgress.percent}%` : ''}` : '') || (vault.tileProgress ? `${vault.tileProgress.phase} · ${vault.tileProgress.percent}%` : vault.busy ? 'Операция с хранилищем…' : 'Нет активных загрузок'))}</button><button className={section === 'settings' ? 'active' : ''} aria-current={section === 'settings' ? 'page' : undefined} onClick={() => setSection('settings')}><Settings size={16}/>{tr("Настройки")}</button></footer>
    {tr(epic.accountError && <div className="account-notice" role="alert">{tr(epic.accountError)}<span>{tr("Вход доступен в меню аккаунта справа сверху.")}</span></div>)}
    {section !== 'settings' && <AppUpdateNotice updates={appUpdates} onOpen={() => setSection('settings')}/>}
    <ClosePrompt/>{tr(epic.dialog)}{tr(vault.dialog)}{tr(about && <AboutDialog onClose={() => setAbout(false)}/>)}
  </div>;
}
function title(s: Section) { return ({ home: 'Главная', engines: 'Unreal Engine', projects: 'Проекты', library: 'Библиотека', plugins: 'Плагины', updates: 'Обновления', downloads: 'Загрузки', settings: 'Настройки', redeem: 'Активировать ключ' } as const)[s]; }
function HomeView({ engines, projects, downloaded, onGo, onRemoved }: {
    downloaded: number;
    engines: EngineInstall[];
    projects: UnrealProject[];
    onGo: (s: Section) => void;
    onRemoved: (path: string) => void;
}) {
    const [launchError, setLaunchError] = useState('');
    return <div className="content home-content"><div className="hero home-hero"><div><span className="eyebrow">{tr("РАБОЧАЯ СРЕДА")}</span><h2>{tr("Всё для Unreal Engine.")}<br />{tr("В одном месте.")}</h2><p>{tr("Ваши движки, проекты, материалы и плагины — под рукой.")}</p><div className="row"><button className="primary" onClick={() => onGo('projects')}><FolderKanban size={17}/>{tr("Мои проекты")}</button><button onClick={() => onGo('library')}><Library size={17}/>{tr("Библиотека Fab")}</button></div></div><img src={logo} alt={tr("")} className="home-hero-logo"/></div><div className="stats home-stats"><Stat n={engines.length} t="Установлено движков"/><Stat n={projects.length} t="Найдено проектов"/><Stat n={downloaded} t="Скачано в хранилище"/></div> <section><div className="section-head"><div><h3>{tr("Последние проекты")}</h3></div><button onClick={() => onGo('projects')}>{tr("Все проекты")}</button></div>{tr(launchError && <p className="error-text" role="alert">{tr(launchError)}</p>)}{tr(projects.length > 0 ? <div className="fab-grid recent-projects">{tr(projects.slice(0, 5).map(p => <ProjectTile key={p.path} project={p} onError={setLaunchError} onRemoved={onRemoved}/>))}</div> : <Empty text="Проекты пока не найдены"/>)}</section>
 </div>;
}
function ProjectsView({ projects, query, setQuery, refresh, onRemoved }: {
    projects: UnrealProject[];
    query: string;
    setQuery: (s: string) => void;
    refresh: () => Promise<void>;
    onRemoved: (path: string) => void;
}) { const [launchError, setLaunchError] = useState(''); return <div className="content"><div className="toolbar"><label className="search"><Search size={17}/><input placeholder={tr("Поиск проектов")} value={query} onChange={e => setQuery(e.target.value)}/></label><PageActions><button onClick={refresh}><RefreshCw size={17}/>{tr("Сканировать")}</button></PageActions></div>{tr(launchError.trim() && <p className="error-text" role="alert">{tr(launchError)}</p>)}<div className="fab-grid recent-projects">{tr(projects.map(p => <ProjectTile key={p.path} project={p} onError={setLaunchError} onRemoved={onRemoved}/>))}{tr(projects.length === 0 && <Empty text="Проекты не найдены. Поиск выполняется в стандартных каталогах Unreal Projects."/>)}</div></div>; }
function SettingsView({ vaultSettings, appUpdates }: {
    vaultSettings: React.ReactNode;
    appUpdates: ReturnType<typeof useAppUpdates>;
}) { return <div className="content settings"><LanguageSettings /><AppUpdateSettings updates={appUpdates}/><WindowSettings /><DownloadSettings /><CacheSettings />{tr(vaultSettings)}<section><span className="eyebrow">{tr("ПРОИЗВОДИТЕЛЬНОСТЬ")}</span><h3>{tr("Графика интерфейса")}</h3><div className="setting-row"><div><b>{tr("Программная отрисовка")}</b><span>{tr("Аппаратное ускорение WebView2 отключено. Состояние дискретной GPU проверяется внешним мониторингом.")}</span></div><span className="status-ok">{tr("ВКЛЮЧЕНО")}</span></div><div className="setting-row"><div><b>{tr("Анимации интерфейса")}</b><span>{tr("Только короткие событийные transform/opacity переходы. В простое анимации отсутствуют.")}</span></div><span>{tr("Экономичные")}</span></div><div className="setting-row"><div><b>{tr("Фоновая отрисовка")}</b><span>{tr("Скрытое окно не выполняет постоянные визуальные обновления.")}</span></div><span className="status-ok">{tr("Отключена")}</span></div></section></div>; }
function Placeholder({ icon, title, text }: {
    icon: any;
    title: string;
    text: string;
}) { return <div className="content"><div className="placeholder">{tr(icon)}<h2>{tr(title)}</h2><p>{tr(text)}</p><span>{tr("Каркас раздела готов для подключения Epic Provider.")}</span></div></div>; }
function Stat({ n, t }: {
    n: number;
    t: string;
}) { return <div className="stat"><strong>{tr(n)}</strong><span>{tr(t)}</span></div>; }
function Empty({ text }: {
    text: string;
}) { return <div className="empty">{tr(text)}</div>; }
