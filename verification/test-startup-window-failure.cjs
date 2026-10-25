// Real main.tsx failure/retry path, failure screen, header and ClosePrompt.
// Healthy App, IPC, native window/events and settings are synthetic; no profile or account is used.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const current=path.resolve(__dirname,'current'),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
assert(parent===current||parent.startsWith(current+path.sep),'Evidence must stay under verification/current');
const id=new Date().toISOString().replace(/[:.]/g,'-')+'-'+crypto.randomUUID().slice(0,8);
const evidence=path.join(parent,'startup-window-failure-'+id),temporary=path.join(current,'sfi-'+crypto.randomUUID().slice(0,8));
assert.equal(path.dirname(temporary),current);assert.match(path.basename(temporary),/^sfi-[0-9a-f]{8}$/);
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY=evidence;process.env.TEMP=temporary;process.env.TMP=temporary;
const {root,esbuild,playwright,browserLaunchOptions}=require('./test-env.cjs');
const languages=['ru','en','de','it','es','fr','zh','ja','ko'];
const report={schema:1,mockedNative:true,actualStartupEntry:true,healthyAppStubbed:true,realSettingsProfileAccess:false,nativeCloseAcceptance:false,checks:[],errors:[],externalRequests:[],temporary};
const check=(name,extra={})=>report.checks.push({name,passed:true,...extra});
const core=`export class Channel{constructor(callback){this.onmessage=callback;}}
export async function invoke(name,args={}){
 window.ipcCalls.push({name,args});
 if(name==='settings_read'){
  if(window.readMode==='fail')throw Error(window.fixture.error);
  if(window.readMode==='hold')return new Promise((resolve,reject)=>{window.pendingRead={resolve,reject};});
  return {...window.fixture.settings};
 }
 if(name==='settings_write'){window.disk[args.key]=args.value;return;}
 if(name==='set_ui_language'||name==='window_close_action')return;
 throw Error('Unexpected synthetic startup IPC: '+name);
}`;
const events=`const listeners=new Map();
window.emitNative=(name,payload)=>{for(const fn of listeners.get(name)||[])fn({event:name,id:1,payload});};
export async function listen(name,fn){const set=listeners.get(name)||new Set();set.add(fn);listeners.set(name,set);window.closeListenerCount=listeners.get('um-close-request')?.size||0;return()=>{set.delete(fn);window.closeListenerCount=listeners.get('um-close-request')?.size||0;};}`;
const nativeWindow=`const resized=new Set();
const call=async(name,args)=>{window.windowCalls.push({name,args});};
const current={onResized:async fn=>{resized.add(fn);return()=>resized.delete(fn);},isMaximized:async()=>!!window.maximized,
 minimize:()=>call('minimize'),close:async()=>{await call('close');window.emitNative('um-close-request','');},
 toggleMaximize:async()=>{await call('toggleMaximize');window.maximized=!window.maximized;for(const fn of resized)fn({payload:{width:1280,height:800}});},
 startDragging:()=>call('startDragging'),setTheme:theme=>call('setTheme',theme),setBackgroundColor:color=>call('setBackgroundColor',color)};
export const getCurrentWindow=()=>current;`;
const healthyApp=`import React from'react';export default function App(){return <main id="healthy-app-fixture">Synthetic healthy application mounted</main>;}`;
const entry="import './src/main';import{tr}from'./src/i18n';window.translate=tr;";
const flush=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
async function main(){
 const built=await esbuild.build({stdin:{contents:entry,resolveDir:root,loader:'tsx'},bundle:true,write:false,outfile:'startup-fixture.js',format:'iife',logLevel:'silent',define:{'process.env.NODE_ENV':'"development"'},loader:{'.svg':'dataurl'},plugins:[{name:'synthetic-startup-native',setup(build){
  build.onResolve({filter:/^@tauri-apps\/api\/(core|event|window)$/},args=>({path:args.path,namespace:'startup-native'}));
  build.onLoad({filter:/.*/,namespace:'startup-native'},args=>({contents:args.path.endsWith('/core')?core:args.path.endsWith('/event')?events:nativeWindow,loader:'js'}));
  build.onResolve({filter:/^\.\/App$/},args=>args.importer.replace(/\\/g,'/').endsWith('/src/main.tsx')?{path:'healthy-app',namespace:'startup-healthy'}:undefined);
  build.onLoad({filter:/.*/,namespace:'startup-healthy'},()=>({contents:healthyApp,loader:'jsx',resolveDir:root}));
 }}]});
 const code=built.outputFiles.find(file=>file.path.endsWith('.js')).text,css=built.outputFiles.filter(file=>file.path.endsWith('.css')).map(file=>file.text).join('\n');
 const browser=await playwright.chromium.launch(browserLaunchOptions);
 async function open(locale='ru',theme='dark'){
  const context=await browser.newContext({viewport:{width:1280,height:800},locale:'ru-RU',reducedMotion:'reduce'});
  await context.route('**/*',route=>{if(route.request().url()==='https://um-startup-failure.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div id="root"></div></body></html>'});report.externalRequests.push(route.request().url());return route.abort();});
  const page=await context.newPage();page.setDefaultTimeout(8000);page.on('pageerror',error=>report.errors.push(error.message));await page.goto('https://um-startup-failure.invalid/');
  await page.evaluate(f=>{window.fixture=f;window.readMode='fail';window.pendingRead=null;window.__TAURI_INTERNALS__={};window.ipcCalls=[];window.windowCalls=[];window.disk={};localStorage.setItem('unreal-manager.language.v1',f.locale);localStorage.setItem('unreal-manager.theme.v1',f.theme);localStorage.setItem('fixture-unrelated','preserve');},
   {locale,theme,error:'Fixture settings failure <img src="fixture-injected.invalid" onerror="window.startupInjected=true">',settings:{'unreal-manager.language.v1':locale,'unreal-manager.theme.v1':theme}});
  await page.addScriptTag({content:code});await page.locator('.startup-failure').waitFor();await flush(page);return{page,context};
 }
 const text=(page,source)=>page.evaluate(source=>window.translate(source),source);
 const button=async(page,source)=>page.getByRole('button',{name:await text(page,source),exact:true});
 const callCount=(page,name)=>page.evaluate(name=>window.ipcCalls.filter(call=>call.name===name).length,name);
 async function fitting(page){
  for(const locator of [page.locator('.workspace-topbar'),page.locator('.window-controls')])assert(await locator.evaluate(node=>{const r=node.getBoundingClientRect();return r.width>0&&r.height>0&&r.left>=-1&&r.right<=innerWidth+1&&r.top>=-1&&r.bottom<=innerHeight+1;}),'Failure chrome is not reachable');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'Failure page overflows horizontally');
 }
 try{
  const sourceFiles=['src/main.tsx','src/AppStartupFailure.tsx','src/WindowSettings.tsx','src/WorkspaceHeader.tsx','src/WorkspaceHeader.css','src/theme.ts','src/preferences.ts','src/i18n.ts','src/locales/workspace.json'];
  report.source=Object.fromEntries(sourceFiles.map(file=>[file,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex')]));
  for(const locale of languages)for(const theme of ['dark','light']){
   const{page,context}=await open(locale,theme);assert.equal(await callCount(page,'settings_read'),1);assert.equal(await callCount(page,'settings_write'),0,'Failed native restore overwrote cached preferences');
   await page.getByRole('heading',{name:await text(page,'Не удалось загрузить настройки'),exact:true}).waitFor();assert((await page.getByRole('alert').innerText()).includes('Fixture settings failure'));
   assert.equal(await page.locator('.startup-failure img').count(),0);assert.equal(await page.evaluate(()=>!!window.startupInjected),false,'Native error was interpreted as HTML');
   assert.equal(await page.locator('#healthy-app-fixture').count(),0);assert.equal(await page.evaluate(()=>document.documentElement.dataset.theme),theme);assert.equal(await page.evaluate(()=>localStorage.getItem('unreal-manager.theme.v1')),theme);assert.equal(await page.evaluate(()=>localStorage.getItem('fixture-unrelated')),'preserve');
   await fitting(page);await page.setViewportSize({width:600,height:400});await flush(page);await fitting(page);await page.setViewportSize({width:1280,height:800});await flush(page);
   await page.waitForFunction(()=>window.closeListenerCount===1);await page.evaluate(()=>window.windowCalls=[]);
   await(await button(page,'Свернуть окно')).click();await(await button(page,'Развернуть окно')).click();await(await button(page,'Восстановить окно')).click();
   assert.deepEqual(await page.evaluate(()=>window.windowCalls.map(call=>call.name)),['minimize','toggleMaximize','toggleMaximize']);
   await(await button(page,'Закрыть окно')).click();const prompt=page.getByRole('dialog',{name:await text(page,'Закрыть приложение?'),exact:true});await prompt.waitFor();assert(await prompt.evaluate(node=>node.contains(document.activeElement)),'Ask dialog did not receive focus');
   await prompt.getByRole('button',{name:await text(page,'Отмена'),exact:true}).click();await prompt.waitFor({state:'detached'});assert.equal(await callCount(page,'window_close_action'),0,'Cancel sent a close/tray action');assert.equal(await page.locator('.startup-failure').isVisible(),true);
   await(await button(page,'Закрыть окно')).click();await prompt.waitFor();await prompt.getByRole('button',{name:await text(page,'Закрыть приложение'),exact:true}).click();await prompt.waitFor({state:'detached'});
   assert.deepEqual(await page.evaluate(()=>window.ipcCalls.filter(call=>call.name==='window_close_action').map(call=>call.args)),[{action:'exit',remember:false}]);
   if(locale==='ru')await page.screenshot({path:path.join(evidence,'startup-failure-'+theme+'.png')});
   await page.evaluate(()=>window.readMode='hold');const retry=await button(page,'Повторить');await retry.click();await page.waitForFunction(()=>!!window.pendingRead);const reads=await callCount(page,'settings_read');assert.equal(reads,2);
   await page.evaluate(()=>{for(const element of document.querySelectorAll('button'))if(element.textContent===window.translate('Повторить')){element.click();element.click();}});await flush(page);assert.equal(await callCount(page,'settings_read'),reads,'Repeated Retry started concurrent settings reads');await fitting(page);
   await page.evaluate(()=>{const pending=window.pendingRead;window.pendingRead=null;pending.resolve(window.fixture.settings);});await page.locator('#healthy-app-fixture').waitFor();assert.equal(await page.locator('#healthy-app-fixture').count(),1);assert.equal(await page.locator('.startup-failure').count(),0);
   await page.waitForFunction(()=>window.closeListenerCount===0);assert.equal(await callCount(page,'settings_read'),2);assert.equal(await page.evaluate(()=>localStorage.getItem('unreal-manager.theme.v1')),theme);assert.equal(await page.evaluate(()=>localStorage.getItem('fixture-unrelated')),'preserve');
   check('Actual bootstrap settings failure retains chrome/Ask close flow and one guarded retry restores the app',{locale,theme});await context.close();
  }
  assert.deepEqual(report.errors,[]);assert.deepEqual(report.externalRequests,[]);report.passed=true;
 }finally{await browser.close();fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));}
 console.log('PASS '+report.checks.length+' actual startup-failure locale/theme scenarios. Window/events/settings and healthy App are synthetic. Evidence: '+evidence);
}
main().catch(error=>{report.passed=false;report.failure=error.stack||String(error);fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));console.error(error);process.exitCode=1;});
