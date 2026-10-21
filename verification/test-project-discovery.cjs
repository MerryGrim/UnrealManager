// Actual ProjectsView + ProjectTile with a synthetic native provider. No host
// directories, project descriptors, profile, scanner or application is opened.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const current=path.resolve(__dirname,'current'),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
assert(parent===current||parent.startsWith(current+path.sep),'Evidence must remain under verification/current');
const id=new Date().toISOString().replace(/[:.]/g,'-')+'-'+crypto.randomUUID().slice(0,8);
const evidence=path.join(parent,'project-discovery-'+id),temporary=path.join(current,'pdi-'+crypto.randomUUID().slice(0,8));
assert.equal(path.dirname(temporary),current);assert.match(path.basename(temporary),/^pdi-[0-9a-f]{8}$/);
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY=evidence;process.env.TEMP=temporary;process.env.TMP=temporary;
const environment=require('./test-env.cjs'),{root,browserLaunchOptions}=environment;
const alpha={name:'Project Alpha',path:'X:/Fixture/Projects/Alpha/Alpha.uproject',engineAssociation:'5.8',modifiedUnix:10};
const beta={name:'Project Beta',path:'X:/Fixture/Chosen/Nested/Beta/Beta.uproject',engineAssociation:'5.8',modifiedUnix:20};
const selectedRoot='X:/Fixture/Chosen';
const core=`export class Channel{constructor(callback){this.onmessage=callback;}}
export async function invoke(name,args={}){
 const f=window.fixture;window.calls.push({name,id:args.id,root:args.root,folder:args.folder,hasProgress:!!args.onProgress});
 if(name==='set_ui_language'||name==='settings_write')return;
 if(name==='choose_project_location')return f.chosenRoot;
 if(name==='scan_projects')return new Promise((resolve,reject)=>{window.pendingRefresh={resolve,reject};});
 if(name==='project_discovery_scan'){
  if(window.pendingScan)throw Error('A duplicate synthetic scan started');
  return new Promise((resolve,reject)=>{window.pendingScan={id:args.id,root:args.root,progress:args.onProgress,resolve,reject};});
 }
 if(name==='project_discovery_cancel')return !!window.pendingScan&&args.id===window.pendingScan.id;
 window.unexpected.push(name);throw Error('Unexpected project fixture IPC '+name);
}`;
const entry=`import React from'react';import{createRoot}from'react-dom/client';import{ProjectsView}from'./src/ProjectsView';import{PageActionsTarget}from'./src/PageActions';import{scanProjects}from'./src/lib/backend';import{setLanguagePreference,useLanguage}from'./src/i18n';
window.chooseLanguage=setLanguagePreference;setLanguagePreference(window.fixture.locale||'ru');
function Harness(){useLanguage();const[projects,setProjects]=React.useState(window.fixture.projects||[]),[initialLoading,setInitialLoading]=React.useState(!!window.fixture.initialLoading),[target,setTarget]=React.useState(null);
 window.replaceProjects=setProjects;window.setInitialLoading=setInitialLoading;
 const refresh=async()=>{const rows=await scanProjects();setProjects(rows);return rows;};
 return <><header className="fixture-actions" ref={setTarget}/><PageActionsTarget.Provider value={target}><ProjectsView projects={projects} onRefresh={refresh} initialLoading={initialLoading} onRemoved={path=>setProjects(rows=>rows.filter(row=>row.path!==path))}/></PageActionsTarget.Provider></>;
}createRoot(document.getElementById('root')).render(<React.StrictMode><Harness/></React.StrictMode>);`;
const report={schema:1,mockedNative:true,realFilesystemDiscoveryVerified:false,checks:[],cases:[],externalRequests:[],errors:[]};
const check=(name,details={})=>report.checks.push({name,passed:true,...details});
async function main(){
 const {esbuild,playwright}=environment;
 const built=await esbuild.build({stdin:{contents:entry,resolveDir:root,loader:'tsx'},bundle:true,write:false,define:{'process.env.NODE_ENV':'"development"'},loader:{'.svg':'dataurl'},plugins:[{name:'project-discovery-fixture',setup(build){build.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'core',namespace:'project-fixture'}));build.onLoad({filter:/.*/,namespace:'project-fixture'},()=>({contents:core,loader:'js'}));}}]});
 const browser=await playwright.chromium.launch(browserLaunchOptions),css=fs.readFileSync(path.join(root,'src/styles.css'),'utf8');
 const dictionaries=JSON.parse(fs.readFileSync(path.join(root,'src/locales/project-discovery.json'),'utf8'));
 const text=(source,locale='ru')=>locale==='ru'?source:dictionaries[locale][source];
 const flush=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 const status=page=>page.locator('.projects-discovery-status');
 const callCount=(page,name)=>page.evaluate(name=>window.calls.filter(call=>call.name===name).length,name);
 async function open(fixture={}){
  const context=await browser.newContext({viewport:{width:1280,height:800},locale:'ru-RU',reducedMotion:'reduce'});
  await context.route('**/*',route=>{if(route.request().url()==='https://um-project-discovery.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div id="root"></div></body></html>'});report.externalRequests.push(route.request().url());return route.abort();});
  const page=await context.newPage();page.on('pageerror',e=>report.errors.push(e.message));await page.goto('https://um-project-discovery.invalid/');
  await page.evaluate(f=>{window.fixture={chosenRoot:'X:/Fixture/Chosen',...f};window.calls=[];window.unexpected=[];window.pendingScan=null;window.pendingRefresh=null;},fixture);
  await page.addScriptTag({content:built.outputFiles[0].text});await flush(page);return{page,context};
 }
 async function close(context,page){assert.deepEqual(await page.evaluate(()=>window.unexpected),[]);report.cases.push({calls:await page.evaluate(()=>window.calls)});await context.close();}
 async function finishRefresh(page,rows){await page.waitForFunction(()=>!!window.pendingRefresh);await page.evaluate(rows=>{const pending=window.pendingRefresh;window.pendingRefresh=null;pending.resolve(rows);},rows);await flush(page);}
 async function begin(page,locale='ru'){await page.getByRole('button',{name:text('Найти проекты',locale),exact:true}).click();await page.waitForFunction(()=>!!window.pendingScan);return page.evaluate(()=>window.pendingScan.id);}
 async function progress(page,extra={}){await page.evaluate(extra=>{const pending=window.pendingScan;pending.progress.onmessage({id:pending.id,directories:3,scannedEntries:12,found:0,skipped:1,currentDirectory:'X:/Fixture/Chosen/Nested',phase:'scan',...extra});},extra);await flush(page);}
 async function finish(page,extra={}){await page.evaluate(extra=>{const pending=window.pendingScan;window.pendingScan=null;pending.resolve({id:pending.id,root:pending.root,directories:3,scannedEntries:12,found:0,added:0,skipped:1,cancelled:false,limited:false,limitReason:null,errors:[],...extra});},extra);await flush(page);}
 try{
  {const{page,context}=await open({projects:[],initialLoading:true});await status(page).getByText('Обновляем список проектов…',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isDisabled(),true);await page.evaluate(()=>window.setInitialLoading(false));await flush(page);assert.equal(await page.locator('.project-tile').count(),0);assert.equal(await callCount(page,'project_discovery_scan'),0);check('Initial empty loading has visible status and never starts an unrequested drive scan');await close(context,page);}
  {const{page,context}=await open({projects:[alpha]});await page.getByRole('button',{name:'Обновить список',exact:true}).click();await status(page).getByText('Обновляем список проектов…',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isDisabled(),true);await finishRefresh(page,[alpha]);await status(page).getByText('Список проектов обновлён.',{exact:true}).waitFor();assert.equal(await page.locator('.project-tile').count(),1);assert.equal(await callCount(page,'scan_projects'),1);check('Unchanged known-project refresh visibly completes instead of appearing to do nothing');await close(context,page);}
  {const{page,context}=await open({chosenRoot:null,projects:[alpha]});await page.getByRole('button',{name:'Найти проекты',exact:true}).click();await flush(page);assert.equal(await callCount(page,'choose_project_location'),1);assert.equal(await callCount(page,'project_discovery_scan'),0);assert.equal(await page.locator('.project-tile').count(),1);assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isEnabled(),true);check('Cancelling the native folder picker does not scan or clear known projects');await close(context,page);}
  {const{page,context}=await open({projects:[alpha]});const query=page.getByPlaceholder('Поиск проектов',{exact:true});await query.fill('Alpha');const id=await begin(page);assert.match(id,/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);await status(page).getByText('Ищем проекты…',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isDisabled(),true);assert.equal(await page.getByRole('button',{name:'Обновить список',exact:true}).isDisabled(),true);await page.getByRole('button',{name:'Найти проекты',exact:true}).evaluate(button=>button.click());assert.equal(await callCount(page,'project_discovery_scan'),1);await progress(page,{found:1});assert((await status(page).innerText()).includes('X:/Fixture/Chosen/Nested'));const before=await status(page).innerText();await progress(page,{id:'11111111-1111-4111-8111-111111111111',found:999,currentDirectory:'X:/Fixture/Stale'});assert.equal(await status(page).innerText(),before);const args=await page.evaluate(()=>window.calls.find(call=>call.name==='project_discovery_scan'));assert.equal(args.root,selectedRoot);assert.equal(args.hasProgress,true);check('One explicit chosen-root scan uses UUID/channel, disables duplicate actions and ignores stale progress');await finish(page,{found:1,added:1});await finishRefresh(page,[alpha,beta]);await status(page).getByText('Поиск завершён.',{exact:true}).waitFor();assert.equal(await query.inputValue(),'Alpha');assert.equal(await page.locator('.project-tile').count(),1);await query.fill('');assert.equal(await page.locator('.project-tile').count(),2);check('Newly discovered projects appear after the native result while the current search filter is preserved');await page.screenshot({path:path.join(evidence,'completed-projects.png')});await close(context,page);}
  {const{page,context}=await open({projects:[alpha]});const id=await begin(page);await progress(page,{found:1});await page.getByRole('button',{name:'Остановить поиск',exact:true}).click();await status(page).getByRole('status').filter({hasText:'Останавливаем поиск…'}).waitFor();assert.equal(await page.evaluate(()=>window.calls.find(call=>call.name==='project_discovery_cancel').id),id);assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isDisabled(),true);await finish(page,{found:1,added:1,cancelled:true});await finishRefresh(page,[alpha,beta]);await status(page).getByText('Поиск остановлен. Найденные проекты сохранены.',{exact:true}).waitFor();assert.equal(await page.locator('.project-tile').count(),2);assert.equal(await callCount(page,'project_discovery_cancel'),1);check('Cancellation targets the active UUID, waits for completion and retains old plus partial discoveries');await close(context,page);}
  {
   const{page,context}=await open({projects:[alpha]});await begin(page);await progress(page,{phase:'save',found:1});
   await status(page).getByText('Сохраняем найденные проекты…',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Остановить поиск',exact:true}).isDisabled(),true);
   await page.getByRole('button',{name:'Остановить поиск',exact:true}).evaluate(button=>button.click());assert.equal(await callCount(page,'project_discovery_cancel'),0);
   await finish(page,{found:1,added:1});await status(page).getByRole('status').filter({hasText:'Обновляем список проектов…'}).waitFor();assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isDisabled(),true);
   await finishRefresh(page,[alpha,beta]);await status(page).getByText('Поиск завершён.',{exact:true}).waitFor();check('Saving cannot be interrupted and a received result remains visibly busy until catalogue refresh finishes');await close(context,page);
  }
  {const{page,context}=await open({projects:[alpha]});await page.getByPlaceholder('Поиск проектов',{exact:true}).fill('Alpha');await begin(page);await page.evaluate(()=>{const pending=window.pendingScan;window.pendingScan=null;pending.reject(Error('simulated unreadable folder'));});await page.getByRole('alert').filter({hasText:'simulated unreadable folder'}).waitFor();assert.equal(await page.locator('.project-tile').count(),1);assert.equal(await page.getByPlaceholder('Поиск проектов',{exact:true}).inputValue(),'Alpha');assert.equal(await callCount(page,'project_discovery_scan'),1);assert.equal(await callCount(page,'scan_projects'),0);assert.equal(await page.getByRole('button',{name:'Найти проекты',exact:true}).isEnabled(),true);check('Scanner failure preserves the existing list/query and does not retry or replace results');await close(context,page);}
  {
   const{page,context}=await open({projects:[alpha]});await begin(page);await finish(page,{found:1,added:1});await page.waitForFunction(()=>!!window.pendingRefresh);
   await page.evaluate(()=>{const pending=window.pendingRefresh;window.pendingRefresh=null;pending.reject(Error('simulated catalogue refresh failure'));});
   await page.getByRole('alert').filter({hasText:'simulated catalogue refresh failure'}).waitFor();await page.getByText('Найденные проекты сохранены, но список не удалось обновить. Нажмите «Обновить список».',{exact:true}).waitFor();assert.equal(await page.locator('.project-tile').count(),1);
   await page.getByRole('button',{name:'Обновить список',exact:true}).click();await finishRefresh(page,[alpha,beta]);assert.equal(await page.locator('.project-tile').count(),2);assert.equal(await page.getByRole('alert').count(),0);check('A saved discovery with failed follow-up refresh retains the prior list and can be recovered by explicit refresh');await close(context,page);
  }
  {const{page,context}=await open({projects:[alpha]});await begin(page);await finish(page,{found:1,added:1,limited:true,limitReason:'entries',errors:['simulated skipped directory']});await finishRefresh(page,[alpha,beta]);await status(page).getByText('Поиск ограничен. Можно выбрать более узкую папку.',{exact:true}).waitFor();assert.equal(await page.locator('.project-tile').count(),2);await page.locator('.projects-scan-errors summary').click();await page.getByText('simulated skipped directory',{exact:true}).waitFor();check('A finite scan limit is reported as limited, with retained discoveries and accessible warning context');await close(context,page);}
  for(const locale of ['ru','en','de','it','es','fr','zh','ja','ko']){const{page,context}=await open({locale,projects:[]});await begin(page,locale);await status(page).getByText(text('Ищем проекты…',locale),{exact:true}).waitFor();await progress(page);await finish(page);await finishRefresh(page,[]);await status(page).getByText(text('Поиск завершён.',locale),{exact:true}).waitFor();assert.equal(await page.locator('.project-tile').count(),0);const body=await status(page).innerText();if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(body),'No untranslated Russian discovery status in '+locale);check('Visible busy and unchanged zero-result completion in '+locale);await close(context,page);}
  assert.deepEqual(report.errors,[]);assert.deepEqual(report.externalRequests,[]);report.passed=true;
 }finally{await browser.close();}
}
main().catch(error=>{report.passed=false;report.failure=error.stack;process.exitCode=1;}).finally(()=>{
 try{if(path.dirname(temporary)!==current||fs.lstatSync(temporary).isSymbolicLink())throw Error('Unexpected transient cleanup target');fs.rmSync(temporary,{recursive:true,force:true,maxRetries:10,retryDelay:100});}catch(error){report.cleanupFailure=error.stack;report.passed=false;process.exitCode=1;}
 report.temporaryRemoved=!fs.existsSync(temporary);
 report.scope='Headless actual React components with synthetic channels/results only. Does not prove filesystem discovery, persistence, drive traversal, permissions or native cancellation.';
 report.sources=['src/ProjectsView.tsx','src/App.tsx','src/lib/backend.ts','src/ProjectTile.tsx','src/i18n.ts','src/locales/project-discovery.json'].filter(name=>fs.existsSync(path.join(root,name))).map(name=>({path:'source/'+name,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(root,name))).digest('hex')}));
 fs.writeFileSync(path.join(evidence,'ui-report.json'),JSON.stringify(report,null,2),{flag:'wx'});console.log((report.passed?'PASS':'FAIL')+' '+report.checks.length+' project discovery UI checks; evidence '+evidence);if(report.failure)console.error(report.failure);if(report.cleanupFailure)console.error(report.cleanupFailure);
});
