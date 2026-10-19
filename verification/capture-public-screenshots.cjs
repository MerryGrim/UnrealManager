// Public screenshots of the real React UI with entirely fabricated data.
// No native app, Epic login, external library, user profile, or transfer is used.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');

const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const evidenceDirectory = path.join(__dirname, 'current', `public-screenshots-${stamp}`);
const publicDirectory = process.argv.includes('--preview')
  ? path.join(evidenceDirectory, 'preview')
  : path.resolve(__dirname, '..', 'docs', 'assets', 'screenshots');
const temporaryDirectory = path.join(evidenceDirectory, 'browser-temp');
for (const directory of [evidenceDirectory, publicDirectory, temporaryDirectory]) {
  fs.mkdirSync(directory, { recursive: true });
}
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY = evidenceDirectory;
process.env.TEMP = temporaryDirectory;
process.env.TMP = temporaryDirectory;
const { root, esbuild, playwright, browserLaunchOptions } = require('./test-env.cjs');
const sharpPath = process.env.UM_SHARP_MODULE || require.resolve('sharp', {
  paths: [root, __dirname, ...(process.env.UM_PLAYWRIGHT_MODULE ? [path.dirname(process.env.UM_PLAYWRIGHT_MODULE)] : [])],
});
const sharp = require(sharpPath);

const GiB = 1024 ** 3;
const demoItems = [
  ['landscape', 'Demo Landscape Study', 'Asset', 'Nature', '#71918d'],
  ['workshop', 'Demo Modular Workshop', 'Asset', 'Buildings & Architecture', '#9c866b'],
  ['materials', 'Demo Material Samples', 'Asset', 'Materials & Textures', '#8a7d9c'],
  ['lighting', 'Demo Lighting Toolkit', 'Plugin', 'Tools & Plugins', '#788ca1'],
  ['exploration', 'Demo Exploration Project', 'Project', 'Environments', '#819a77'],
  ['widgets', 'Demo UI Widgets', 'Asset', 'UI & Interfaces', '#a48c86'],
].map(([id, title, kind, category, color], index) => ({
  id: `demo-${id}`, title, kind, categories: [category], engineVersions: ['5.7', '5.6'],
  source: 'Fab', cover: `demo://${id}`, color,
  versions: [{ artifactId: `demo-artifact-${index}`, engines: ['5.7'], platforms: ['Windows'] }],
}));
const demoPackages = demoItems.slice(1, 2).map((item, index) => ({
  id: `demo-package-${index}`, assetId: item.id, title: item.title, kind: item.kind,
  engines: ['5.7'], bytes: (index ? 0.8 : 2.4) * GiB, downloadBytes: GiB,
  files: 128 + index * 25, path: `X:/Fixture/Demo/UnrealManager/Vault/${item.id}`,
}));
const activePackage = {
  id: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', assetId: demoItems[0].id,
  artifactId: 'demo-artifact-0', selectedEngine: '5.7', buildVersion: 'demo-build',
  title: demoItems[0].title, kind: 'Asset', engines: ['5.7'], bytes: 4 * GiB,
  downloadBytes: 2.4 * GiB, files: 1240, path: 'X:/Fixture/Demo/UnrealManager/Vault/demo-landscape',
};
const demoEngines = [
  { name: 'Unreal Engine', version: '5.7.2', path: 'X:/Fixture/Demo/Unreal/UE_5.7', source: 'demo' },
  { name: 'Unreal Engine', version: '5.6.1', path: 'X:/Fixture/Demo/Unreal/UE_5.6', source: 'demo' },
];

function makeCover(item, index) {
  // Self-authored static placeholders, not real Fab artwork or product covers.
  const shapes = index % 2
    ? '<path d="M90 210V112l85-47 85 47v98l-85 47zM175 65v192M90 112l85 47 85-47M175 159v98" fill="none" stroke="#eef2f4" stroke-width="3" opacity=".65"/><path d="M275 250V155l65-36 65 36v95l-65 36zM340 119v167M275 155l65 36 65-36" fill="none" stroke="#eef2f4" stroke-width="2" opacity=".25"/>'
    : '<path d="M0 240 115 95 235 240 337 127 512 268V320H0z" fill="#e7efef" opacity=".15"/><path d="M0 280 142 174 268 268 384 184 512 290V320H0z" fill="#f4f7f6" opacity=".22"/><circle cx="385" cy="78" r="28" fill="#edf4f4" opacity=".55"/>';
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="320" viewBox="0 0 512 320"><rect width="512" height="320" fill="#1c2832"/><rect width="512" height="320" fill="${item.color}" opacity=".4"/>${shapes}<text x="22" y="300" font-family="Segoe UI,Arial,sans-serif" font-size="15" letter-spacing="3" fill="#fff" opacity=".65">DEMO ASSET</text></svg>`;
  return 'data:image/svg+xml;base64,' + Buffer.from(svg).toString('base64');
}
const covers = Object.fromEntries(demoItems.map((item, index) => [item.id, makeCover(item, index)]));

const mockCore = `
const items=${JSON.stringify(demoItems)};
const packages=${JSON.stringify(demoPackages)};
const activePackage=${JSON.stringify(activePackage)};
const engines=${JSON.stringify(demoEngines)};
const covers=${JSON.stringify(covers)};
export class Channel { constructor(callback){this.onmessage=callback;} }
export async function invoke(name,args={}) {
  window.demoCalls.push(name);
  switch(name) {
    case 'epic_status':return {signedIn:true,displayName:'Demo User'};
    case 'scan_engines':return engines;
    case 'scan_projects':return [{name:'Demo Studio',path:'X:/Fixture/Demo/Projects/DemoStudio/DemoStudio.uproject',engineAssociation:'5.7',modifiedUnix:1792368000}];
    case 'engine_catalog':return [
      {appName:'UE_5.8',version:'5.8',build:'5.8.0-demo',installed:[]},
      {appName:'UE_5.7',version:'5.7',build:'5.7.3-demo',installed:[{path:engines[0].path,version:'5.7.2',build:'5.7.2-demo',updateAvailable:true}]},
      {appName:'UE_5.6',version:'5.6',build:'5.6.1-demo',installed:[{path:engines[1].path,version:'5.6.1',build:'5.6.1-demo',updateAvailable:false}]}
    ];
    case 'engine_pending':case 'engine_resume_tasks':return [];
    case 'vault_list':return packages;
    case 'vault_resume_tasks':return window.demoTransferStarted?[]:[{id:activePackage.id,assetId:activePackage.assetId,title:activePackage.title,kind:'Asset',selectedEngine:'5.7',path:activePackage.path,phase:'На паузе',done:2*1024**3,total:4*1024**3,filesDone:620,filesTotal:1240,intent:{mode:'vault',project:'',projectName:'',engineVersion:'5.7'}}];
    case 'vault_epic_status':return {directory:'X:/Fixture/Demo/Epic/VaultCache',items:[],warnings:[]};
    case 'fab_cached_library':return {items,cachedAt:1792368000,stale:false};
    case 'fab_library':return {items,scope:'fictional-public-demo'};
    case 'fab_cover':return covers[args.id]||'';
    case 'vault_resume_prepare':return activePackage;
    case 'vault_download':
      window.demoTransferStarted=true;
      window.demoProgressChannel=args.onProgress;
      window.demoProgress={phase:'Загружаем и проверяем файлы',done:2.52*1024**3,total:4*1024**3,filesDone:781,filesTotal:1240,networkBytes:1.51*1024**3,bytesPerSecond:24*1024**2,paused:false,canPause:true};
      args.onProgress.onmessage(window.demoProgress);
      return new Promise(()=>{});
    case 'vault_pause':
      window.demoProgress={...window.demoProgress,paused:args.paused,bytesPerSecond:args.paused?0:24*1024**2};
      window.demoProgressChannel.onmessage(window.demoProgress);return;
    case 'close_behavior_get':return 'ask';
    case 'app_update_settings_get':return {enabled:true,channel:'beta',configured:true,currentVersion:${JSON.stringify(require('../source/package.json').version)},mode:'portable',supported:true};
    case 'app_update_check':return {status:'current',release:null,checkedAt:1792396800,cached:true};
    case 'download_limit_get':return 32*1024**2;
    case 'project_association_get':return {enabled:true,windowsChoiceRequired:false};
    case 'vault_storage_get':return {vaultPath:'X:/Fixture/Demo/UnrealManager/Vault',keepEngineBackup:false};
    case 'storage_info':return {dataPath:'X:/Fixture/Demo/UnrealManager/Data',cachePath:'X:/Fixture/Demo/UnrealManager/Data/Cache',settingsPath:'X:/Fixture/Demo/UnrealManager/Data/Settings',cacheBytes:84*1024**2};
    case 'settings_read':return {'unreal-manager.language.v1':'en'};
    case 'settings_write':case 'set_ui_language':case 'engine_discard':return;
    default:window.unexpectedDemoCalls.push(name);throw Error('Unsupported fictional screenshot command: '+name);
  }
}`;

async function main() {
  const result = await esbuild.build({
    stdin: {contents: "import React from 'react';import {createRoot} from 'react-dom/client';import App from './src/App';import {setLanguagePreference} from './src/i18n';setLanguagePreference('en');createRoot(document.getElementById('root')).render(<App/>);", resolveDir: root, loader: 'tsx'},
    bundle: true, write: false, loader: {'.svg': 'dataurl'},
    plugins: [{name: 'fictional-native-api', setup(build) {
      build.onResolve({filter: /^@tauri-apps\/api\/(core|event)$/}, args => ({path: args.path, namespace: 'demo'}));
      build.onLoad({filter: /.*/, namespace: 'demo'}, args => ({
        contents: args.path.endsWith('/event') ? 'export async function listen(){return ()=>{};}' : mockCore,
        loader: 'js',
      }));
    }}],
  });
  const errors = [];
  const requests = [];
  const browser = await playwright.chromium.launch({
    ...browserLaunchOptions,
    args: [...browserLaunchOptions.args, '--disable-background-networking', '--disable-sync'],
  });
  try {
    const context = await browser.newContext({locale: 'en-US', viewport: {width:1280,height:800}, deviceScaleFactor:1, reducedMotion:'reduce', colorScheme:'dark'});
    const css = fs.readFileSync(path.join(root, 'src', 'styles.css'), 'utf8');
    await context.route('**/*', route => {
      const url = route.request().url();
      requests.push(url);
      if (url === 'http://um-public.invalid/') {
        return route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="en"><head><meta charset="UTF-8"><style>${css}</style></head><body><div id="root"></div></body></html>`});
      }
      return route.abort();
    });
    const page = await context.newPage();
    page.on('pageerror', error => errors.push(error.message));
    await page.goto('http://um-public.invalid/');
    await page.evaluate(() => {
      window.demoCalls=[];window.unexpectedDemoCalls=[];
      localStorage.setItem('unreal-manager.language.v1','en');
      localStorage.setItem('um-library-page-size','24');
    });
    await page.addScriptTag({content:result.outputFiles[0].text});
    await page.getByRole('button',{name:'Account menu',exact:true}).waitFor();
    const captures=[];
    async function capture(name) {
      await page.mouse.move(1275,795);
      await page.evaluate(async()=>{await document.fonts.ready;await Promise.all(Array.from(document.images).map(image=>image.complete?undefined:new Promise(resolve=>{image.onload=resolve;image.onerror=resolve;})));});
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
      assert.deepEqual(await page.evaluate(()=>window.unexpectedDemoCalls),[]);
      assert.deepEqual(errors,[]);
      const png=path.join(evidenceDirectory,`${name}.png`);
      const webp=path.join(publicDirectory,`${name}.webp`);
      await page.screenshot({path:png,animations:'disabled'});
      await sharp(png).webp({lossless:true,effort:6}).toFile(webp);
      const metadata=await sharp(webp).metadata();
      assert.equal(metadata.width,1280);assert.equal(metadata.height,800);
      captures.push({name,width:1280,height:800,locale:'en',theme:'dark',publicFile:path.relative(path.join(__dirname,'..'),webp).replaceAll('\\','/'),sha256:createHash('sha256').update(fs.readFileSync(webp)).digest('hex')});
      console.log(`Captured ${name}: 1280x800 lossless WebP`);
    }
    await page.getByRole('button',{name:'Engines',exact:true}).click();
    await page.locator('.card.engine').first().waitFor();
    await page.getByRole('button',{name:'Launch',exact:true}).first().waitFor();
    await capture('engines');
    await page.getByRole('button',{name:'Library',exact:true}).click();
    await page.locator('.fab-tile').last().waitFor();
    await page.waitForFunction(()=>document.querySelectorAll('.fab-cover img').length>=6);
    await capture('library');
    await page.getByRole('button',{name:'Settings',exact:true}).click();
    await page.getByRole('tab',{name:'App updates',exact:true}).click();
    await page.locator('.settings-panel[data-settings-category="appupdates"]').waitFor();
    await page.locator('#app-update-channel:not(:disabled)').waitFor();
    await capture('settings');
    await page.locator('.workspace-footer button').first().click();
    await page.getByRole('button',{name:'Resume download',exact:true}).click();
    await page.locator('.vault-transfer-card').first().getByRole('button',{name:'Pause',exact:true}).waitFor();
    await capture('downloads');
    fs.writeFileSync(path.join(evidenceDirectory,'capture-manifest.json'),JSON.stringify({
      capturedAt:new Date().toISOString(),sourceRoot:root,
      method:'Actual React App with mocked Tauri commands, fabricated account/library/projects/paths and simulated transfer; no Epic service or native app used.',
      artwork:'Self-authored static SVG demo placeholders, not Fab product artwork.',
      theme:'The application supports dark appearance. No fabricated light application theme was added.',
      softwareBrowserArguments:browserLaunchOptions.args,
      actualGpuActivity:'Not measured. Browser configuration is not hardware telemetry.',
      requests,errors,captures,
    },null,2)+'\n');
    console.log(`Evidence: ${evidenceDirectory}`);
  } finally { await browser.close(); }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
