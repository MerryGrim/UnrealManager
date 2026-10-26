const fs = require('node:fs');
const assert = require('node:assert/strict');
const env = require('./test-env.cjs');

const catalogueItem = {
  id: 'complete-project', title: 'Complete Project fixture', kind: 'Asset',
  engineVersions: ['5.8'], categories: [], source: 'Fab',
  versions: [{artifactId: 'project-artifact', engines: ['5.8'], platforms: ['Windows']}],
};
const nativeMock = `
export class Channel {constructor(callback) {this.onmessage = callback;}}
function packageInfo(kind = 'Project') {return {
  id: 'fixture-package', assetId: 'complete-project', title: 'Complete Project fixture',
  artifactId: 'project-artifact', selectedEngine: '5.8', kind, engines: ['5.8'],
  bytes: 100, downloadBytes: 100, files: 3, path: 'X:/Fixture/Vault/fixture-package'
};}
export async function invoke(name, args) {
  window.calls.push({name, args});
  if (name === 'vault_list') return window.finished ? [packageInfo(window.downloadedKind)] : window.fixture.packages || [];
  if (name === 'vault_resume_tasks') return window.fixture.resume ? [{
    ...packageInfo('Asset'), selectedEngine: '5.8', phase: 'На паузе', done: 50, total: 100,
    filesDone: 1, filesTotal: 3, intent: {mode: 'install', project: 'X:/Fixture/Existing.uproject', projectName: 'Existing', engineVersion: '5.8'}
  }] : [];
  if (name === 'vault_prepare' || name === 'vault_resume_prepare') return packageInfo(window.fixture.preparedKind || 'Project');
  if (name === 'vault_discard') return;
  if (name === 'vault_queue_get') return {revision: 1, paused: false, activeId: null, items: [], completed: null};
  if (name === 'vault_storage_get') return {vaultPath: 'X:/Fixture/Vault'};
  if (name === 'vault_epic_status') return {directory: '', items: [], warnings: []};
  if (name === 'engine_resume_tasks') return [];
  if (name === 'fab_cached_library') return {items: window.fixture.catalogue, cachedAt: 1, stale: !!window.fixture.syncTypes};
  if (name === 'fab_library') {
    window.libraryProgress = args.onProgress;
    return new Promise(resolve => window.finishLibrary = () => resolve({items: window.fixture.syncedCatalogue || window.fixture.catalogue, scope: 'owned', typeStats: window.fixture.typeStats}));
  }
  if (name === 'fab_details') return {description: '', images: [], author: ''};
  if (name === 'choose_project_location') return 'X:/Fixture/New';
  if (name === 'vault_prepared_preview' || name === 'vault_install_preview') return {
    target: args.project, projectFile: args.project + '/' + (args.projectName || 'Existing') + '.uproject',
    files: 3, conflicts: [], existing: 0, compatible: true
  };
  if (name === 'vault_download') {window.finished = true; return packageInfo(window.fixture.downloadedKind || window.fixture.preparedKind || 'Project');}
  if (name === 'vault_install') return 3;
  throw Error('Unexpected mocked native command: ' + name);
}
`;
const eventMock = `export async function listen() {return () => {};}`;
const harness = `
import React from 'react';
import {createRoot} from 'react-dom/client';
import {useVault} from './src/Vault';
import {FabLibrary} from './src/EpicLibrary';
import {PageActionsTarget} from './src/PageActions';
function Harness() {
  const vault = useVault([{name: 'Existing', path: 'X:/Fixture/Existing.uproject', engineAssociation: '5.8'}], () => {}, []);
  window.vault = vault;
  if (window.fixture.library) return <PageActionsTarget.Provider value={document.getElementById('actions')}><FabLibrary account={{signedIn: true, displayName: 'Fixture'}}
    onLogin={() => {}} onDownload={item => window.openedItem = item} downloadBusy={false}
    downloadProgress={null} refreshSignal={0} downloaded={window.fixture.packages || []} onVaultChanged={async () => {}}/></PageActionsTarget.Provider>;
  return <><button onClick={() => vault.select(window.fixture.catalogue[0], true)}>Open fixture</button>
    <button onClick={() => vault.openInstall(window.fixture.staleHistory)}>Install history fixture</button>
    {vault.dialog}{vault.view}</>;
}
createRoot(document.getElementById('root')).render(<Harness/>);
`;

(async () => {
  const bundle = await env.esbuild.build({
    stdin: {contents: harness, resolveDir: env.root, loader: 'tsx'}, bundle: true, write: false,
    loader: {'.svg': 'dataurl'}, plugins: [{name: 'mock-native', setup(build) {
      build.onResolve({filter: /^@tauri-apps\/api\/(core|event)$/}, args => ({path: args.path, namespace: 'mock-native'}));
      build.onLoad({filter: /.*/, namespace: 'mock-native'}, args => ({contents: args.path.endsWith('/event') ? eventMock : nativeMock, loader: 'js'}));
    }}],
  });
  const browser = await env.playwright.chromium.launch(env.browserLaunchOptions);
  const checks = [], errors = [];
  try {
    async function open(fixture = {}) {
      const page = await browser.newPage({locale: 'ru-RU', viewport: {width: 1280, height: 900}});
      page.on('pageerror', error => errors.push(error.message));
      await page.route('http://um-kind.test/', route => route.fulfill({contentType: 'text/html', body: '<html><head><meta charset="UTF-8"><style>' + fs.readFileSync(env.root + '/src/styles.css', 'utf8') + '</style></head><body><div id="actions"></div><div id="root"></div></body></html>'}));
      await page.goto('http://um-kind.test/');
      await page.evaluate(fixture => {window.fixture = fixture; window.calls = [];}, {catalogue: [catalogueItem], ...fixture});
      await page.addScriptTag({content: bundle.outputFiles[0].text});
      return page;
    }
    const callArgs = (page, name) => page.evaluate(name => window.calls.filter(call => call.name === name).map(call => call.args), name);
    async function chooseNewProject(page) {
      const dialog = page.getByRole('dialog', {name: catalogueItem.title, exact: true});
      assert.equal(await dialog.getByRole('combobox', {name: 'Проект для материала', exact: true}).count(), 0);
      assert.equal(await dialog.getByRole('textbox', {name: 'Назначение материала', exact: true}).inputValue(), '');
      assert(await dialog.getByRole('button', {name: 'Далее', exact: true}).isDisabled());
      await dialog.getByRole('textbox', {name: 'Имя файла проекта', exact: true}).fill('NewProject');
      await dialog.getByRole('textbox', {name: 'Назначение материала', exact: true}).fill('X:/Fixture/NewProject');
      await dialog.getByRole('button', {name: 'Далее', exact: true}).click();
      await dialog.getByRole('button', {name: 'Скачать и создать проект', exact: true}).waitFor();
      assert.deepEqual((await callArgs(page, 'vault_prepared_preview')).at(-1), {id: 'fixture-package', project: 'X:/Fixture/NewProject', projectName: 'NewProject'});
      await dialog.getByRole('button', {name: 'Скачать и создать проект', exact: true}).click();
      await page.waitForFunction(() => window.calls.some(call => call.name === 'vault_install'));
      const installs = await callArgs(page, 'vault_install');
      assert.equal(installs.length, 1);
      assert.equal(installs[0].project, 'X:/Fixture/NewProject');
      assert.equal(installs[0].projectName, 'NewProject');
      assert.equal(installs[0].engineVersion, '5.8');
      assert(!(await callArgs(page, 'vault_prepared_preview')).some(args => args.project.endsWith('Existing.uproject')));
    }

    const fresh = await open();
    await fresh.getByRole('button', {name: 'Open fixture', exact: true}).click();
    await fresh.getByRole('combobox', {name: 'Версия Unreal Engine для материала', exact: true}).selectOption(JSON.stringify(['project-artifact', '5.8']));
    await fresh.getByRole('combobox', {name: 'Проект для материала', exact: true}).selectOption('X:/Fixture/Existing.uproject');
    await fresh.getByRole('button', {name: 'Далее', exact: true}).click();
    await fresh.getByText('Тип материала уточнён по файлам. Выберите место установки заново.', {exact: true}).waitFor();
    assert.equal((await callArgs(fresh, 'vault_prepared_preview')).length, 0);
    assert.equal((await callArgs(fresh, 'vault_download')).length, 0);
    await chooseNewProject(fresh);
    assert.equal((await callArgs(fresh, 'vault_prepare')).length, 1, 'The corrected manifest is reused after explicit new destination');
    checks.push('Asset catalogue + Project manifest requires a new project destination before preview, download or install; prepared metadata is reused');
    await fresh.close();

    const resumed = await open({resume: true});
    await resumed.getByRole('button', {name: 'Продолжить загрузку', exact: true}).click();
    await resumed.getByText('Тип материала уточнён по файлам. Выберите место установки заново.', {exact: true}).first().waitFor();
    assert.equal((await callArgs(resumed, 'vault_download')).length, 0);
    await chooseNewProject(resumed);
    assert.equal((await callArgs(resumed, 'vault_resume_prepare')).length, 1);
    assert.equal((await callArgs(resumed, 'vault_prepare')).length, 0);
    checks.push('An old Asset resume intent targeting an existing .uproject cannot automatically install a newly identified Project');
    await resumed.close();

    const changedOnCompletion = await open({preparedKind: 'Asset', downloadedKind: 'Project'});
    await changedOnCompletion.getByRole('button', {name: 'Open fixture', exact: true}).click();
    await changedOnCompletion.getByRole('combobox', {name: 'Версия Unreal Engine для материала', exact: true}).selectOption(JSON.stringify(['project-artifact', '5.8']));
    await changedOnCompletion.getByRole('combobox', {name: 'Проект для материала', exact: true}).selectOption('X:/Fixture/Existing.uproject');
    await changedOnCompletion.getByRole('button', {name: 'Далее', exact: true}).click();
    await changedOnCompletion.getByRole('button', {name: 'Скачать и добавить в проект', exact: true}).click();
    const correctedInstall = changedOnCompletion.getByRole('dialog', {name: catalogueItem.title, exact: true});
    await correctedInstall.getByRole('textbox', {name: 'Имя файла проекта', exact: true}).waitFor();
    assert.equal(await correctedInstall.getByRole('textbox', {name: 'Путь установки', exact: true}).inputValue(), '');
    assert.equal((await callArgs(changedOnCompletion, 'vault_install')).length, 0);
    checks.push('A final package kind different from the prepared kind stays in Vault and opens a new explicit destination flow instead of applying an old target');
    await changedOnCompletion.close();

    const pkg = {id: 'fixture-package', assetId: catalogueItem.id, title: catalogueItem.title, kind: 'Project', engines: ['5.8'], bytes: 100, files: 3, path: 'X:/Fixture/Vault/fixture-package'};
    const historical = await open({packages: [pkg], staleHistory: {...pkg, kind: 'Asset'}});
    await historical.waitForFunction(() => window.vault.items.length === 1);
    await historical.getByRole('button', {name: 'Install history fixture', exact: true}).click();
    const historyDialog = historical.getByRole('dialog', {name: catalogueItem.title, exact: true});
    await historyDialog.getByRole('textbox', {name: 'Имя файла проекта', exact: true}).waitFor();
    assert.equal(await historyDialog.locator('select').count(), 0);
    checks.push('An old download-history Asset label uses the corrected completed Vault package when opened for installation');
    await historical.close();

    const library = await open({library: true, packages: [pkg]});
    await library.getByRole('combobox', {name: 'Тип материала', exact: true}).selectOption('Project');
    const tile = library.getByRole('button', {name: 'Открыть ' + catalogueItem.title, exact: true});
    await tile.waitFor();
    await tile.click({button: 'right'});
    await library.getByRole('menuitem', {name: 'Создать проект', exact: true}).waitFor();
    assert.equal(await library.getByRole('menuitem', {name: 'Добавить в проект', exact: true}).count(), 0);
    await library.getByRole('menuitem', {name: 'Информация', exact: true}).click();
    const details = library.getByRole('dialog', {name: 'Информация: ' + catalogueItem.title, exact: true});
    await details.getByRole('button', {name: 'Создать проект', exact: true}).waitFor();
    await details.getByRole('button', {name: 'Создать проект', exact: true}).click();
    assert.equal(await library.evaluate(() => window.openedItem.kind), 'Project');
    checks.push('Completed Vault Project metadata corrects catalogue filters, card actions, details and the selected action without a library refresh');
    await library.close();

    const ambiguous = await open({library: true, packages: [pkg, {...pkg, id: 'asset-variant', kind: 'Asset'}]});
    await ambiguous.getByRole('combobox', {name: 'Тип материала', exact: true}).selectOption('Asset');
    await ambiguous.getByRole('button', {name: 'Открыть ' + catalogueItem.title, exact: true}).waitFor();
    await ambiguous.getByRole('combobox', {name: 'Тип материала', exact: true}).selectOption('Project');
    await ambiguous.getByText('Материалы не найдены.', {exact: true}).waitFor();
    checks.push('Mixed Asset/Project package versions do not impose one kind on the entire catalogue item');
    await ambiguous.close();

    const unmatched = await open({library: true, packages: [{...pkg, assetId: 'another-item'}]});
    await unmatched.getByRole('combobox', {name: 'Тип материала', exact: true}).selectOption('Asset');
    await unmatched.getByRole('button', {name: 'Открыть ' + catalogueItem.title, exact: true}).waitFor();
    checks.push('A same-title package with another assetId cannot change a catalogue item kind');
    await unmatched.close();

    const declaredPlugin = await open({library: true, catalogue: [{...catalogueItem, kind: 'Plugin'}], packages: [pkg]});
    await declaredPlugin.getByRole('combobox', {name: 'Тип материала', exact: true}).selectOption('Plugin');
    await declaredPlugin.getByRole('button', {name: 'Открыть ' + catalogueItem.title, exact: true}).waitFor();
    checks.push('A declared Plugin is not globally reclassified as Project by the local catalogue overlay');
    await declaredPlugin.close();

    const owned = [
      {...catalogueItem, title: 'Multiplayer Shooter System PACK fixture', kind: 'Project'},
      {...catalogueItem, id: 'undownloaded-project', title: 'Another undownloaded project fixture', kind: 'Project'},
      {...catalogueItem, id: 'code-plugin', title: 'CODE_PLUGIN fixture', kind: 'Plugin'},
      ...Array.from({length: 1990}, (_, index) => ({...catalogueItem, id: 'owned-asset-' + index, title: 'Owned asset fixture ' + index, kind: 'Asset'})),
    ];
    const synced = await open({library: true, syncTypes: true, packages: [], catalogue: owned.map(item => ({...item, kind: 'Asset'})), syncedCatalogue: owned, typeStats: {total: 1440, done: 1440, unresolved: 3, fetched: 1200, cached: 240}});
    await synced.waitForFunction(() => !!window.libraryProgress);
    const status = synced.locator('.library-loading');
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'pages', pages: 20, items: 1993}));
    await status.getByText('Получено материалов: 1993 · Страниц: 20', {exact: true}).waitFor();
    const pageHeight = (await status.boundingBox()).height;
    assert.equal(await status.getByRole('progressbar').getAttribute('value'), null, 'Page collection must not invent a completion percentage');
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'types', pages: 20, items: 1993, done: 720, total: 1440, unresolved: 0}));
    await status.getByText('Уточняем типы материалов…', {exact: true}).waitFor();
    await status.getByText('Проверено типов: 720 / 1440', {exact: true}).waitFor();
    assert.equal(await status.getByRole('progressbar').getAttribute('value'), '720');
    assert.equal(await status.getByRole('progressbar').getAttribute('max'), '1440');
    assert.equal((await status.boundingBox()).height, pageHeight, 'Changing the native sync phase must retain the progress area height');
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'types', pages: 20, items: 1993, done: 900, total: 1440, unresolved: 3}));
    await status.getByText('Не удалось уточнить: 3', {exact: true}).waitFor();
    assert.equal((await status.boundingBox()).height, pageHeight, 'A native unresolved counter must use the reserved progress line');
    checks.push('Page collection and all-owned type metadata sync show native counters in one stable progress area; unresolved counts do not move its layout');
    await synced.evaluate(() => window.finishLibrary());
    await status.waitFor({state: 'hidden'});
    await synced.getByText('Не удалось уточнить типы материалов: 3. Повторите обновление библиотеки.', {exact: true}).waitFor();
    assert((await synced.locator('.library-summary').innerText()).includes('1993 материалов'));
    assert((await synced.locator('.downloaded-filter').innerText()).includes('(0)'));
    const typeFilter = synced.getByRole('combobox', {name: 'Тип материала', exact: true});
    await typeFilter.selectOption('Project');
    await synced.getByRole('button', {name: 'Открыть Multiplayer Shooter System PACK fixture', exact: true}).waitFor();
    await synced.getByRole('button', {name: 'Открыть Another undownloaded project fixture', exact: true}).waitFor();
    assert.equal(await synced.locator('.fab-tile').count(), 2);
    await synced.getByRole('button', {name: 'Открыть Multiplayer Shooter System PACK fixture', exact: true}).click({button: 'right'});
    await synced.getByRole('menuitem', {name: 'Создать проект', exact: true}).waitFor();
    await synced.keyboard.press('Escape');
    await typeFilter.selectOption('Plugin');
    await synced.getByRole('button', {name: 'Открыть CODE_PLUGIN fixture', exact: true}).waitFor();
    assert.equal(await synced.locator('.fab-tile').count(), 1);
    await typeFilter.selectOption('Asset');
    assert((await synced.locator('.library-pagination').innerText()).includes('Найдено: 1990'));
    await typeFilter.selectOption('All');
    assert((await synced.locator('.library-pagination').innerText()).includes('Найдено: 1993'));
    assert.equal(await synced.evaluate(() => window.calls.some(call => ['vault_prepare', 'vault_download', 'vault_install'].includes(call.name))), false);
    checks.push('With an empty Vault, the synced native library independently exposes two undownloaded Projects and a CODE_PLUGIN while retaining 1993 owned entries and correct type filters');
    await synced.getByRole('button', {name: 'Обновить библиотеку', exact: true}).click();
    await synced.waitForFunction(() => window.calls.filter(call => call.name === 'fab_library').length === 2);
    await status.waitFor();
    assert.equal(await synced.getByText('Не удалось уточнить типы материалов: 3. Повторите обновление библиотеки.', {exact: true}).count(), 0);
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'waiting', pages: 0, items: 0}));
    await status.getByText('Ожидаем предыдущую синхронизацию…', {exact: true}).waitFor();
    const waitingHeight = (await status.boundingBox()).height;
    assert.equal(await status.getByRole('progressbar').count(), 0, 'A queued sync must not display invented page/type progress');
    assert.equal(await status.locator('p').innerText(), '\u00a0');
    assert((await synced.locator('.library-summary').innerText()).includes('1993 материалов'), 'Waiting for the prior sync must retain the owned library');
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'pages', pages: 1, items: 100}));
    await status.getByText('Получено материалов: 100 · Страниц: 1', {exact: true}).waitFor();
    assert.equal((await status.boundingBox()).height, waitingHeight, 'Starting the queued native sync must not resize its progress area');
    assert.equal(await status.getByRole('progressbar').getAttribute('value'), null);
    await synced.evaluate(() => window.libraryProgress.onmessage({phase: 'types', pages: 20, items: 1993, done: 1440, total: 1440, unresolved: 0}));
    await status.getByText('Проверено типов: 1440 / 1440', {exact: true}).waitFor();
    assert.equal((await status.boundingBox()).height, waitingHeight);
    assert.equal(await status.getByRole('progressbar').getAttribute('value'), '1440');
    checks.push('A queued native sync shows a stable waiting notice without invented progress, retains the owned library and resumes real page/type counters without a layout jump');
    await synced.evaluate(() => {window.fixture.typeStats = {total: 1440, done: 1440, unresolved: 0, fetched: 3, cached: 1437}; window.finishLibrary();});
    await status.waitFor({state: 'hidden'});
    assert.equal(await synced.getByText('Не удалось уточнить типы материалов: 3. Повторите обновление библиотеки.', {exact: true}).count(), 0);
    assert((await synced.locator('.library-summary').innerText()).includes('1993 материалов'));
    checks.push('A partial type sync retains the owned items and a concise retry notice; a later successful sync clears the notice without losing the catalogue');
    await synced.close();
    assert.deepEqual(errors, []);
    fs.writeFileSync(env.outputDirectory + '/fab-project-kind-ui-report.json', JSON.stringify({passed: true, checks, scope: 'Actual React components with synthetic native replies; no real account, Vault, project or transfer accessed.'}, null, 2));
    process.stdout.write('PASS: ' + checks.length + ' Fab full-project UI regressions\n');
  } finally {await browser.close();}
})().catch(error => {console.error(error); process.exitCode = 1;});
