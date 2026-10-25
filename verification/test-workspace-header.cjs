// Actual header, account menu, locale and theme modules; all window/IPC calls are synthetic.
// This is renderer geometry/keyboard proof, not native Windows DPI, Epic login or measured GPU proof.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const current=path.resolve(__dirname,'current'),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
assert(parent===current||parent.startsWith(current+path.sep),'Evidence must stay under verification/current');
const stamp=new Date().toISOString().replace(/[:.]/g,'-')+'-'+crypto.randomUUID().slice(0,8);
const evidence=path.join(parent,'workspace-header-'+stamp),temporary=path.join(current,'whi-'+crypto.randomUUID().slice(0,8));
assert.equal(path.dirname(temporary),current);assert.match(path.basename(temporary),/^whi-[0-9a-f]{8}$/);
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY=evidence;process.env.TEMP=temporary;process.env.TMP=temporary;
const {root,esbuild,playwright,browserLaunchOptions}=require('./test-env.cjs');
const report={schema:1,mockedNative:true,nativeDpiAcceptance:false,physicalGpuMeasurement:false,checks:[],geometry:[],errors:[],externalRequests:[],temporary};
const check=(name,extra={})=>report.checks.push({name,passed:true,...extra});
const languages=['ru','en','de','it','es','fr','zh','ja','ko'];
const core=`export class Channel{constructor(callback){this.onmessage=callback;}}
export async function invoke(name,args={}){window.ipcCalls.push({name,args});if(name==='settings_write'){window.disk[args.key]=args.value;return;}if(name==='set_ui_language')return;if(name==='settings_read')return {...window.disk};throw Error('Unexpected synthetic header IPC: '+name);}`;
const nativeWindow=`const handlers=new Set();
const call=async(name,args)=>{window.windowCalls.push({name,args});if(window.failWindowAction===name)throw Error('Synthetic window failure');};
const current={
 onResized:async fn=>{handlers.add(fn);window.activeResizeListeners=handlers.size;return()=>{handlers.delete(fn);window.activeResizeListeners=handlers.size;};},
 isMaximized:async()=>!!window.fixtureMaximized,
 minimize:()=>call('minimize'),close:()=>call('close'),
 startDragging:()=>call('startDragging'),
 toggleMaximize:async()=>{await call('toggleMaximize');window.fixtureMaximized=!window.fixtureMaximized;for(const fn of handlers)fn({payload:{width:1280,height:800}});},
 setTheme:value=>call('setTheme',value),setBackgroundColor:value=>call('setBackgroundColor',value),
};export const getCurrentWindow=()=>current;`;
const entry=`import React from'react';import{createRoot}from'react-dom/client';
import{WorkspaceHeader}from'./src/WorkspaceHeader';import{AccountMenu}from'./src/AccountMenu';
import{ThemeSettings}from'./src/ThemeSettings';
import{initializeTheme,useTheme}from'./src/theme';import{setLanguagePreference,useLanguage,tr}from'./src/i18n';
import{Boxes,FolderKanban,Library,PlugZap,RefreshCw}from'lucide-react';
initializeTheme();setLanguagePreference(window.fixture.locale);window.translate=tr;window.chooseLanguage=setLanguagePreference;
const items=[{id:'engines',label:'Движки',icon:Boxes},{id:'projects',label:'Проекты',icon:FolderKanban},{id:'library',label:'Библиотека',icon:Library},{id:'plugins',label:'Плагины',icon:PlugZap},{id:'updates',label:'Обновления',icon:RefreshCw,badge:12}];
function Harness(){useLanguage();const{theme}=useTheme(),[section,setSection]=React.useState('library'),[account,setAccount]=React.useState(window.fixture.account);window.changeAccount=setAccount;
const menu=<AccountMenu account={account} busy={false} onLogin={()=>window.accountCalls.push('login')} onLogout={async()=>{window.accountCalls.push('logout');setAccount({signedIn:false,displayName:null});}} onRedeem={()=>window.accountCalls.push('redeem')} onAbout={()=>window.accountCalls.push('about')}/>;
return <div className="app-shell"><WorkspaceHeader section={section} items={items} onNavigate={id=>{window.navigationCalls.push(id);setSection(id);}} accountMenu={menu}/><main className="main workspace-main"><header className="workspace-heading"><h1>{tr('Библиотека')}</h1></header><div className="workspace-page"><div className="content fixture-specimen"><section className="settings"><ThemeSettings/><section><h3>{tr('Настройки')}</h3><div className="setting-row"><label htmlFor="fixture-field">{tr('Название')}</label><input id="fixture-field" defaultValue="Synthetic project"/></div><div className="setting-row"><label htmlFor="fixture-select">{tr('Язык интерфейса')}</label><select id="fixture-select"><option>{tr('Русский')}</option></select></div><p className="muted fixture-muted">{tr('Нет активных загрузок')}</p><p className="error-text fixture-error">{tr('Ошибка')}</p><p className="status-ok fixture-success">{tr('Успешно')}</p><button className="primary fixture-primary">{tr('Обновить список')}</button></section></section><div className="stat"><strong>3</strong><span>{tr('Найдено проектов')}</span></div><output id="fixture-theme">{theme}</output></div></div></main><footer className="workspace-footer"><button>{tr('Нет активных загрузок')}</button><button>{tr('Настройки')}</button></footer></div>}
const tree=createRoot(document.getElementById('root'));window.unmount=()=>tree.unmount();tree.render(<React.StrictMode><Harness/></React.StrictMode>);`;
const flush=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
async function main(){
 const built=await esbuild.build({stdin:{contents:entry,resolveDir:root,loader:'tsx'},bundle:true,write:false,outfile:'header-fixture.js',define:{'process.env.NODE_ENV':'"development"'},loader:{'.svg':'dataurl'},plugins:[{name:'synthetic-header-native',setup(build){build.onResolve({filter:/^@tauri-apps\/api\/(core|window)$/},args=>({path:args.path,namespace:'header-fixture'}));build.onLoad({filter:/.*/,namespace:'header-fixture'},args=>({contents:args.path.endsWith('/core')?core:nativeWindow,loader:'js'}));}}]});
 const code=built.outputFiles.find(file=>file.path.endsWith('.js')).text;
 const css=fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/RedeemView.css'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/WorkspaceHeader.css'),'utf8')+'\n'+built.outputFiles.filter(file=>file.path.endsWith('.css')).map(file=>file.text).join('\n');
 const browser=await playwright.chromium.launch(browserLaunchOptions);
 async function open({locale='ru',theme='dark',width=1280,height=800,account={signedIn:true,displayName:'Fixture Account'},maximized=false,dpr=1}={}){
  const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:dpr,locale:'ru-RU',reducedMotion:'reduce'});
  await context.route('**/*',route=>{if(route.request().url()==='https://um-header.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div id="root"></div></body></html>'});report.externalRequests.push(route.request().url());return route.abort();});
  const page=await context.newPage();page.on('pageerror',error=>report.errors.push(error.message));await page.goto('https://um-header.invalid/');
  await page.evaluate(f=>{window.fixture=f;window.__TAURI_INTERNALS__={};window.windowCalls=[];window.ipcCalls=[];window.disk={};window.accountCalls=[];window.navigationCalls=[];window.fixtureMaximized=f.maximized;localStorage.setItem('unreal-manager.theme.v1',f.theme);localStorage.setItem('unreal-manager.language.v1',f.locale);localStorage.setItem('fixture-unrelated','preserve');},{locale,theme,account,maximized});
  await page.addScriptTag({content:code});await page.locator('.workspace-topbar').waitFor();await flush(page);return{page,context};
 }
 const translated=(page,source)=>page.evaluate(source=>window.translate(source),source);
 const labeled=async(page,source)=>page.getByRole('button',{name:await translated(page,source),exact:true});
 async function geometry(page,details){
  const value=await page.evaluate(()=>{
   const box=node=>{const r=node.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
   const header=document.querySelector('.workspace-topbar'),brand=document.querySelector('.workspace-brand'),nav=document.querySelector('.workspace-nav'),account=document.querySelector('.user-trigger');
   const buttons=[...header.querySelectorAll('button')].filter(button=>{const r=button.getBoundingClientRect();return r.width&&r.height&&!nav?.contains(button);});
   const overlap=[];for(let i=0;i<buttons.length;i++)for(let j=i+1;j<buttons.length;j++){const a=box(buttons[i]),b=box(buttons[j]);if(Math.min(a.right,b.right)-Math.max(a.x,b.x)>1&&Math.min(a.bottom,b.bottom)-Math.max(a.y,b.y)>1)overlap.push([buttons[i].getAttribute('aria-label')||buttons[i].textContent,buttons[j].getAttribute('aria-label')||buttons[j].textContent]);}
   return{viewport:{width:innerWidth,height:innerHeight},header:box(header),brand:box(brand),nav:box(nav),account:box(account),controls:buttons.map(button=>({name:button.getAttribute('aria-label')||button.textContent,...box(button)})),overlap,documentOverflow:document.documentElement.scrollWidth>innerWidth+1,headerOverflow:header.scrollWidth>header.clientWidth+1};
  });
  assert(!value.documentOverflow,'Page horizontal overflow '+JSON.stringify(details));assert(!value.headerOverflow,'Header horizontal overflow '+JSON.stringify(details));
  assert.deepEqual(value.overlap,[],'Header controls overlap '+JSON.stringify(details));
  assert(value.header.x>=-1&&value.header.right<=value.viewport.width+1,'Header exceeds viewport');
  assert(value.header.bottom<=value.viewport.height*.55,'Header consumes most of viewport');
  for(const button of value.controls){assert(button.x>=-1&&button.right<=value.viewport.width+1,'Offscreen control '+button.name);assert(button.y>=-1&&button.bottom<=value.header.bottom+1,'Control leaves header '+button.name);assert(button.width>=28&&button.height>=28,'Unusable control size '+button.name);}
  const links=page.locator('.workspace-nav button');assert.equal(await links.count(),5);
  for(let index=0;index<5;index++){const button=links.nth(index);await button.scrollIntoViewIfNeeded();await button.evaluate(node=>{const r=node.getBoundingClientRect(),parent=node.closest('nav').getBoundingClientRect();if(r.left<Math.max(0,parent.left)-1||r.right>Math.min(innerWidth,parent.right)+1)throw Error('Navigation item clipped even after scrolling');});}
  report.geometry.push({...details,...value});
 }
 async function contrast(page,selector,minimum){
  const result=await page.locator(selector).first().evaluate(node=>{
   const rgba=value=>{const m=value.match(/[\d.]+/g)||[];return[Number(m[0]||0),Number(m[1]||0),Number(m[2]||0),m.length>3?Number(m[3]):1];};
   const blend=(a,b)=>{const alpha=a[3]+b[3]*(1-a[3]);return[0,1,2].map(i=>(a[i]*a[3]+b[i]*b[3]*(1-a[3]))/(alpha||1)).concat(alpha);};
   const stack=[];let current=node;while(current){stack.push(rgba(getComputedStyle(current).backgroundColor));current=current.parentElement;}let background=[255,255,255,1];for(const color of stack.reverse())background=blend(color,background);
   const foreground=blend(rgba(getComputedStyle(node).color),background),lum=color=>color.slice(0,3).map(c=>{c/=255;return c<=.04045?c/12.92:Math.pow((c+.055)/1.055,2.4);}).reduce((sum,c,i)=>sum+c*[.2126,.7152,.0722][i],0);
   const a=lum(foreground),b=lum(background);return{foreground,background,ratio:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};
  });assert(result.ratio>=minimum,selector+' contrast '+result.ratio.toFixed(2)+' < '+minimum);return result;
 }
 try{
  const files=['src/WorkspaceHeader.tsx','src/WorkspaceHeader.css','src/AccountMenu.tsx','src/theme.ts','src/ThemeSettings.tsx','src/styles.css','src/RedeemView.css','src/preferences.ts','src/i18n.ts','src/locales/workspace.json','src-tauri/tauri.conf.json','src-tauri/capabilities/main-events.json'];
  report.source=Object.fromEntries(files.filter(file=>fs.existsSync(path.join(root,file))).map(file=>[file,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex')]));
  for(const locale of languages)for(const theme of ['dark','light']){
   const{page,context}=await open({locale,theme});
   assert.equal(await page.locator('#fixture-theme').innerText(),theme);assert.equal(await page.evaluate(()=>document.documentElement.dataset.theme),theme);assert.equal(await page.evaluate(()=>document.documentElement.style.colorScheme),theme);
   const themeAction=theme==='dark'?'Светлая тема':'Тёмная тема';await(labeled(page,themeAction)).then(button=>button.waitFor());
   for(const source of ['Свернуть окно','Развернуть окно','Закрыть окно'])await(await labeled(page,source)).waitFor();
   for(const size of [{width:1920,height:900,scale:1},{width:1280,height:800,scale:1},{width:1024,height:768,scale:1},{width:900,height:600,scale:1},{width:720,height:480,scale:1.25},{width:600,height:400,scale:1.5}]){await page.setViewportSize({width:size.width,height:size.height});await flush(page);await geometry(page,{locale,theme,...size});}
   await page.setViewportSize({width:1280,height:800});await flush(page);
   if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(await page.locator('.workspace-topbar').innerText()),'Untranslated header in '+locale);
   const before=await page.locator('.workspace-topbar').boundingBox();await(await labeled(page,themeAction)).focus();await page.keyboard.press('Enter');await flush(page);
   assert.equal(await page.locator('#fixture-theme').innerText(),theme==='dark'?'light':'dark');assert.equal(await page.evaluate(()=>localStorage.getItem('unreal-manager.theme.v1')),theme==='dark'?'light':'dark');
   assert.equal(await page.evaluate(()=>localStorage.getItem('fixture-unrelated')),'preserve');assert.equal(await page.evaluate(()=>window.disk['unreal-manager.theme.v1']),theme==='dark'?'light':'dark');
   const after=await page.locator('.workspace-topbar').boundingBox();assert.deepEqual(after,before,'Theme change shifts header layout');
   const themeSelect=page.getByRole('combobox',{name:await translated(page,'Оформление'),exact:true});await themeSelect.selectOption(theme);await flush(page);assert.equal(await page.locator('#fixture-theme').innerText(),theme);await(await labeled(page,themeAction)).waitFor();await themeSelect.selectOption(theme==='dark'?'light':'dark');await flush(page);assert.equal(await page.locator('#fixture-theme').innerText(),theme==='dark'?'light':'dark');
   check('Translated header, bounded responsive layout and keyboard persistent theme toggle',{locale,initialTheme:theme,viewports:6});
   if(locale==='ru'&&theme==='dark'){await page.screenshot({path:path.join(evidence,'header-light-1280.png')});await page.reload();await page.evaluate(()=>{window.fixture={locale:'ru',theme:localStorage.getItem('unreal-manager.theme.v1'),account:{signedIn:true,displayName:'Fixture Account'}};window.__TAURI_INTERNALS__={};window.windowCalls=[];window.ipcCalls=[];window.disk={};window.accountCalls=[];window.navigationCalls=[];});await page.addScriptTag({content:code});await flush(page);assert.equal(await page.locator('#fixture-theme').innerText(),'light');check('Selected light theme is restored on a fresh renderer mount without a system watcher');}
   await context.close();
  }
  for(const theme of ['dark','light']){
   const{page,context}=await open({theme});const results={};
   for(const selector of ['.workspace-brand','.workspace-brand small','.workspace-nav button.active','.user-summary b','.user-summary small','.workspace-heading h1','.fixture-muted','.fixture-error','.fixture-success','#fixture-field','#fixture-select','.fixture-primary','.stat span'])results[selector]=await contrast(page,selector,4.5);
   if(theme==='light'){
    const surfaces=await page.evaluate(()=>{
     const fixture=document.createElement('div');fixture.style.cssText='position:absolute;left:-10000px;top:0;width:500px';fixture.innerHTML='<section class="login-dialog"><h2>Fixture</h2></section><button class="fab-tile">Fixture</button><div class="transfer-card">Fixture</div><div class="project-context-menu">Fixture</div><div class="redemption-table-scroll"><table class="redemption-table"><thead><tr><th>Fixture</th></tr></thead><tbody><tr class="redemption-row-success"><td>Fixture</td><td>Fixture</td><td><strong>Success</strong></td></tr><tr class="redemption-row-failed"><td>Fixture</td><td>Fixture</td><td><strong>Failure</strong></td></tr><tr class="redemption-row-unknown"><td>Fixture</td><td>Fixture</td><td><strong>Unknown</strong></td></tr></tbody></table></div>';document.querySelector('.fixture-specimen').append(fixture);
     const result={};for(const selector of ['.login-dialog','.fab-tile','.transfer-card','.project-context-menu','.redemption-table th']){const style=getComputedStyle(fixture.querySelector(selector));result[selector]={backgroundColor:style.backgroundColor,backgroundImage:style.backgroundImage,color:style.color};}return result;
    });
    for(const selector of ['.login-dialog','.fab-tile','.project-context-menu','.redemption-table th']){const rgb=(surfaces[selector].backgroundColor.match(/[\d.]+/g)||[]).map(Number);assert(rgb.length>=3&&rgb.slice(0,3).every(channel=>channel>=200)&&(rgb.length===3||rgb[3]>=.95),selector+' retained a dark or transparent surface');}
    assert(!/rgb\((?:[0-9]|[1-9][0-9]),/.test(surfaces['.transfer-card'].backgroundImage),'Transfer gradient retained a dark stop');
    for(const selector of ['.redemption-row-success td:last-child>strong','.redemption-row-failed td:last-child>strong','.redemption-row-unknown td:last-child>strong'])results[selector]=await contrast(page,selector,4.5);
    report.lightSurfaces=surfaces;check('Login, asset, transfer, context menu and redemption CSS surfaces use readable light tokens');
   }
   await page.locator('.user-trigger').focus();await page.keyboard.press('Enter');await page.getByRole('menu').waitFor();assert(await page.getByRole('menu').evaluate(menu=>menu.contains(document.activeElement)),'Account menu does not receive keyboard focus');
   await page.keyboard.press('End');assert.equal(await page.evaluate(()=>document.activeElement.getAttribute('role')),'menuitem');await page.keyboard.press('Home');
   await page.keyboard.press('Escape');await page.getByRole('menu').waitFor({state:'detached'});assert.equal(await page.locator('.user-trigger').evaluate(button=>document.activeElement===button),true);
   await page.setViewportSize({width:600,height:400});await page.locator('.user-trigger').click();await page.getByRole('menu').waitFor();assert(await page.getByRole('menu').evaluate(menu=>{const r=menu.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+1&&r.top>=0&&r.bottom<=innerHeight+1;}),'Account menu is clipped at zoom equivalent');
   await page.keyboard.press('Escape');await page.setViewportSize({width:1280,height:800});await flush(page);
   await page.evaluate(()=>window.windowCalls=[]);await(await labeled(page,'Свернуть окно')).click();assert.deepEqual(await page.evaluate(()=>window.windowCalls.map(call=>call.name)),['minimize']);
   await(await labeled(page,'Развернуть окно')).click();await(await labeled(page,'Восстановить окно')).waitFor();await(await labeled(page,'Восстановить окно')).click();await(await labeled(page,'Развернуть окно')).waitFor();await(await labeled(page,'Закрыть окно')).click();
   assert.deepEqual(await page.evaluate(()=>window.windowCalls.map(call=>call.name)),['minimize','toggleMaximize','toggleMaximize','close']);
   await page.evaluate(()=>window.windowCalls=[]);await page.locator('.workspace-nav button').first().dblclick();await page.locator('.workspace-brand').dblclick();await(await labeled(page,theme==='dark'?'Светлая тема':'Тёмная тема')).dblclick();await flush(page);assert.equal(await page.evaluate(()=>window.windowCalls.some(call=>['startDragging','toggleMaximize','close'].includes(call.name))),false,'Interactive header controls bubbled to titlebar drag/maximize');
   await page.evaluate(()=>window.windowCalls=[]);await page.locator('.workspace-topbar').dispatchEvent('mousedown',{button:0,detail:1});await flush(page);assert.deepEqual(await page.evaluate(()=>window.windowCalls.map(call=>call.name)),['startDragging']);await page.locator('.workspace-topbar').dispatchEvent('mousedown',{button:0,detail:2});await flush(page);await(await labeled(page,'Восстановить окно')).waitFor();await page.locator('.workspace-topbar').dispatchEvent('mousedown',{button:2,detail:1});await flush(page);assert.deepEqual(await page.evaluate(()=>window.windowCalls.map(call=>call.name)),['startDragging','toggleMaximize']);
   await page.evaluate(()=>window.failWindowAction='minimize');await(await labeled(page,'Свернуть окно')).click();const failure=page.getByRole('alert');await failure.waitFor();assert(!(await failure.innerText()).includes('Synthetic window failure'),'Window UI exposed arbitrary backend details');await page.evaluate(()=>window.failWindowAction=null);await failure.getByRole('button').click();await failure.waitFor({state:'detached'});await(await labeled(page,'Свернуть окно')).click();
   check('Readable surfaces, keyboard account menu, zoom menu containment and isolated window controls',{theme,contrast:results});
   await page.setViewportSize({width:1920,height:900});await page.screenshot({path:path.join(evidence,'header-'+theme+'-1920.png')});assert.equal(await page.evaluate(()=>window.activeResizeListeners),1);await page.evaluate(()=>window.unmount());await flush(page);assert.equal(await page.evaluate(()=>window.activeResizeListeners),0,'Unmount leaked resize subscription');await context.close();
  }
  {const{page,context}=await open({theme:'invalid'});assert.equal(await page.locator('#fixture-theme').innerText(),'dark');await page.evaluate(()=>window.changeAccount({signedIn:false,displayName:null}));await flush(page);await page.locator('.user-trigger').click();await page.getByRole('menu').waitFor();assert.equal(await page.getByRole('menuitem').count(),2);assert.equal(await page.evaluate(()=>window.accountCalls.length),0);check('Invalid theme falls back to dark and opening signed-out account menu does not start authentication');await context.close();}
  assert.deepEqual(report.externalRequests,[]);assert.deepEqual(report.errors,[]);report.passed=true;
 }finally{await browser.close();fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));}
 console.log('PASS '+report.checks.length+' header/theme scenarios; '+report.geometry.length+' locale/theme/viewport layouts. Mocked window calls; native DPI and hardware GPU activity not measured. Evidence: '+evidence);
}
main().catch(error=>{report.passed=false;report.failure=error.stack||String(error);fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));console.error(error);process.exitCode=1;});
