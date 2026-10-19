// Browser-only updater regression tests. Native commands and packages are
// simulated; these checks do not establish that a real update was installed.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const evidence = path.join(__dirname, 'current', `app-updates-${stamp}`);
const temporary = path.join(evidence, 'browser-temp');
fs.mkdirSync(temporary, { recursive: true });
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY = evidence;
process.env.TEMP = temporary;
process.env.TMP = temporary;
const { root, esbuild, playwright, browserLaunchOptions } = require('./test-env.cjs');
const translations = JSON.parse(fs.readFileSync(path.join(root, 'src/locales/app-updates.json'), 'utf8'));
const checkedAt = 1792396800;
const release = { version: '0.2.0-beta.99', notes: 'Fictional regression release.\nNo package is installed.', url: 'https://github.com/MerryGrim/UnrealManager/releases/tag/v0.2.0-beta.99', size: 8000000, staged: false };
const baseSettings = { enabled: true, channel: 'beta', configured: true, currentVersion: '0.2.0-beta.7', mode: 'portable', supported: true };
const core = `
export class Channel { constructor(callback){this.onmessage=callback;} }
export async function invoke(command,args={}) {
 window.nativeCalls.push({command,args});
 const f=window.fixture;
 if(command==='set_ui_language')return;
 if(command==='settings_read')return {};
 if(command==='settings_write')return;
 if(command==='app_update_settings_get'){if(f.settingsError)throw Error(f.settingsError);return {...f.settings};}
 if(command==='app_update_settings_set'){if(f.saveError)throw Error(f.saveError);f.settings.enabled=args.enabled;f.settings.channel=args.channel;return;}
 if(command==='app_update_check'){
   if(f.checkError)throw Error(f.checkError);
   if(!args.manual&&!f.settings.enabled)return {status:'disabled',release:null,checkedAt:0,cached:false};
   return args.manual?(f.manualResult||f.result):f.result;
 }
 if(command==='app_update_download'){
   if(f.downloadError)throw Error(f.downloadError);
   window.downloadProgress=args.onProgress;
   args.onProgress.onmessage({phase:'downloading',received:4000000,total:8000000,percent:50});
   return new Promise(resolve=>{window.completeDownload=()=>resolve({...f.result.release,staged:true});});
 }
 if(command==='app_update_apply'){if(f.applyError)throw Error(f.applyError);window.appliedVersion=args.version;return;}
 throw Error('Unexpected updater UI command: '+command);
}`;

async function main() {
  const built = await esbuild.build({
    stdin: { contents: `import React,{useState} from 'react';import{createRoot}from'react-dom/client';import{AppUpdateSettings,AppUpdateNotice,useAppUpdates}from'./src/AppUpdateSettings';import{setLanguagePreference,useLanguage}from'./src/i18n';window.chooseLanguage=setLanguagePreference;setLanguagePreference('en');function Harness(){useLanguage();const updates=useAppUpdates();const[open,setOpen]=useState(false);return <><button id="show-settings" onClick={()=>setOpen(true)}>Show settings</button><AppUpdateNotice updates={updates} onOpen={()=>setOpen(true)}/>{open&&<AppUpdateSettings updates={updates}/>}</>;}createRoot(document.getElementById('root')).render(<React.StrictMode><Harness/></React.StrictMode>);`, resolveDir: root, loader: 'tsx' },
    bundle: true, write: false,
    plugins: [{ name: 'mock-update-native', setup(build) {
      build.onResolve({ filter: /^@tauri-apps\/api\/core$/ }, args => ({ path: args.path, namespace: 'update-mock' }));
      build.onLoad({ filter: /.*/, namespace: 'update-mock' }, () => ({ contents: core, loader: 'js' }));
    } }],
  });
  const browser = await playwright.chromium.launch(browserLaunchOptions);
  const report = { mockedNative: true, realInstallVerified: false, checks: [], errors: [], externalRequests: [] };
  const css = fs.readFileSync(path.join(root, 'src/styles.css'), 'utf8');
  async function fixturePage(overrides = {}) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'en-US', reducedMotion: 'reduce' });
    await context.route('**/*', route => {
      if (route.request().url() === 'http://um-update.invalid/') return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><meta charset="utf-8"><style>${css}html,body,#root{overflow:auto;height:auto}#root{padding:24px}.app-update-settings{padding:24px;background:var(--panel);max-width:900px;border:1px solid var(--border)}</style></head><body><div id="root"></div></body></html>` });
      report.externalRequests.push(route.request().url());
      return route.abort();
    });
    const page = await context.newPage();
    page.on('pageerror', error => report.errors.push(error.message));
    await page.goto('http://um-update.invalid/');
    await page.evaluate(value => { window.fixture = value; window.nativeCalls = []; }, { settings: { ...baseSettings }, result: { status: 'available', release: { ...release }, checkedAt, cached: false }, ...overrides });
    await page.addScriptTag({ content: built.outputFiles[0].text });
    await page.locator('#show-settings').click();
    await page.locator('#app-update-settings').waitFor();
    await page.waitForFunction(() => !document.querySelector('.app-update-state').textContent.includes('Loading update settings'));
    return { page, context };
  }
  const check = (name, details = {}) => report.checks.push({ name, passed: true, ...details });
  try {
    {
      const { page, context } = await fixturePage();
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_settings_get').length), 1);
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_check' && call.args.manual === false).length), 1);
      await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_check' && call.args.manual === true).length), 1);
      check('StrictMode startup checks once; manual check bypass is explicit');
      for (const locale of ['ru', 'en', 'de', 'it', 'es', 'fr', 'zh', 'ja', 'ko']) {
        await page.evaluate(value => window.chooseLanguage(value), locale);
        const copy = key => locale === 'ru' ? key : translations[locale][key];
        await page.getByRole('heading', { name: copy('Обновления Unreal Manager'), exact: true }).waitFor();
        assert.equal(await page.locator('#app-update-channel option[value=stable]').textContent(), copy('Стабильные версии'));
        assert.equal(await page.getByRole('button', { name: copy('Скачать обновление'), exact: true }).count(), 1);
        assert.equal(await page.locator('#app-updates-enabled').isChecked(), true);
        check('Localized updater controls', { locale });
      }
      await page.evaluate(() => window.chooseLanguage('ru'));
      await page.screenshot({ path: path.join(evidence, 'updates-ru.png') });
      await page.evaluate(() => window.chooseLanguage('en'));
      await page.locator('#app-update-channel').selectOption('stable');
      await page.waitForFunction(() => window.fixture.settings.channel === 'stable');
      assert.equal(await page.getByRole('button', { name: 'Download update', exact: true }).count(), 0);
      await page.locator('#app-updates-enabled').uncheck();
      await page.waitForFunction(() => window.fixture.settings.enabled === false);
      assert.equal(await page.locator('.app-update-notice').count(), 0);
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_check').length), 2);
      check('Channel and automatic preference save without another background check');
      await context.close();
    }
    {
      const { page, context } = await fixturePage();
      await page.getByRole('button', { name: 'Download update', exact: true }).click();
      await page.waitForFunction(() => document.querySelector('.app-update-progress progress')?.value === 50);
      assert.equal(await page.locator('#app-update-channel').isDisabled(), true);
      await page.evaluate(() => window.downloadProgress.onmessage({ phase: 'verifying', received: 8000000, total: 8000000, percent: 100 }));
      await page.getByRole('progressbar', { name: 'Verifying update package…' }).waitFor();
      await page.evaluate(() => window.completeDownload());
      const install = page.getByRole('button', { name: 'Install and restart', exact: true });
      await install.waitFor();
      assert.equal(await page.getByText('The package is verified and ready to install.', { exact: true }).count(), 1);
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_apply').length), 0);
      await install.click();
      const dialog = page.getByRole('dialog', { name: 'Update Unreal Manager?' });
      await dialog.waitFor();
      assert.match(await dialog.textContent(), /safely stopped and saved/);
      assert.equal(await page.evaluate(() => document.activeElement?.textContent), 'Cancel');
      await page.keyboard.press('Escape');
      assert.equal(await dialog.count(), 0);
      await install.click();
      await dialog.getByRole('button', { name: 'Install and restart', exact: true }).click();
      await page.waitForFunction(() => window.appliedVersion === '0.2.0-beta.99');
      assert.equal(await dialog.getByRole('button', { name: 'Install and restart', exact: true }).isDisabled(), true);
      const downloadCalls = await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_download').map(call => Object.keys(call.args).sort()));
      assert.deepEqual(downloadCalls, [['onProgress', 'version']]);
      check('Download progress, staged verification and explicit apply confirmation');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ applyError: 'Synthetic helper failure', result: { status: 'available', release: { ...release, staged: true }, checkedAt, cached: true } });
      await page.getByRole('button', { name: 'Install and restart', exact: true }).click();
      const dialog = page.getByRole('dialog');
      await dialog.getByRole('button', { name: 'Install and restart', exact: true }).click();
      await dialog.getByRole('alert').waitFor();
      assert.equal(await dialog.getByRole('button', { name: 'Install and restart', exact: true }).isEnabled(), true);
      await page.keyboard.press('Escape');
      check('Apply failure remains recoverable and does not report success');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ downloadError: 'Synthetic invalid signature' });
      await page.getByRole('button', { name: 'Download update', exact: true }).click();
      await page.getByRole('alert').waitFor();
      assert.equal(await page.getByRole('button', { name: 'Install and restart', exact: true }).count(), 0);
      assert.equal(await page.getByRole('button', { name: 'Download update', exact: true }).isEnabled(), true);
      check('Unverified download failure cannot enable installation');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ settings: { ...baseSettings, supported: false } });
      assert.equal(await page.getByRole('button', { name: 'Check for updates', exact: true }).isDisabled(), true);
      assert.equal(await page.locator('#app-updates-enabled').isDisabled(), true);
      assert.equal(await page.getByText('Updates are unavailable for this build.', { exact: true }).count(), 1);
      assert.equal(await page.locator('.app-update-notice').count(), 0);
      assert.equal(await page.evaluate(() => window.nativeCalls.filter(call => call.command === 'app_update_check').length), 0);
      check('Development or unsupported builds cannot update or start a network check');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ settingsError: 'Synthetic settings read failure' });
      await page.getByRole('alert').waitFor();
      await page.evaluate(() => { window.fixture.settingsError = ''; });
      await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
      await page.getByRole('button', { name: 'Download update', exact: true }).waitFor();
      assert.equal(await page.getByRole('alert').count(), 0);
      check('Failed initial settings read can be retried manually');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ checkError: 'Synthetic offline response' });
      await page.getByRole('alert').waitFor();
      assert.equal(await page.locator('#app-update-channel').isEnabled(), true);
      await page.evaluate(() => { window.fixture.checkError = ''; window.fixture.saveError = 'Synthetic write failure'; });
      await page.locator('#app-update-channel').selectOption('stable');
      await page.getByText('Could not save update settings.', { exact: true }).waitFor();
      assert.equal(await page.locator('#app-update-channel').inputValue(), 'beta');
      await page.evaluate(() => { window.fixture.saveError = ''; });
      await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
      await page.getByRole('button', { name: 'Download update', exact: true }).waitFor();
      assert.equal(await page.getByRole('alert').count(), 0);
      check('Offline checks and settings write errors remain recoverable');
      await context.close();
    }
    {
      const { page, context } = await fixturePage({ settings: { ...baseSettings, lastApplyError: 'Synthetic rollback: original executable restored.' }, checkError: 'Synthetic offline response after rollback' });
      const failure = page.getByRole('alert');
      await failure.getByText('The previous update did not complete.', { exact: true }).waitFor();
      assert.match(await failure.textContent(), /original executable restored/);
      assert.equal(await page.getByText('Could not check for updates.', { exact: true }).count(), 0);
      await page.evaluate(() => { window.fixture.checkError = ''; });
      await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
      await page.getByRole('button', { name: 'Download update', exact: true }).waitFor();
      assert.equal(await page.getByRole('alert').count(), 0);
      check('Previous apply failure survives restart, takes priority and clears on manual retry');
      await context.close();
    }
    assert.deepEqual(report.errors, []);
    assert.deepEqual(report.externalRequests, []);
    report.passed = true;
    fs.writeFileSync(path.join(evidence, 'ui-report.json'), JSON.stringify(report, null, 2));
    console.log(`PASS ${report.checks.length} updater renderer checks; evidence ${evidence}`);
  } catch (error) {
    report.passed = false;
    report.failure = error.stack;
    fs.writeFileSync(path.join(evidence, 'ui-report.json'), JSON.stringify(report, null, 2));
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
