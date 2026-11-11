// Actual AccountMenu + RedeemView/i18n/theme, synthetic IPC and synthetic keys only.
// No native application, Epic account, entitlement mutation or user key file is used.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const current=path.resolve(__dirname,'current'),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
assert(parent===current||parent.startsWith(current+path.sep),'Evidence must remain under verification/current');
const evidence=path.join(parent,'redemption-wizard-'+new Date().toISOString().replace(/[:.]/g,'-')+'-'+crypto.randomUUID().slice(0,8));
const temporary=path.join(current,'rww-'+crypto.randomUUID().slice(0,8));
assert.equal(path.dirname(temporary),current);assert.match(path.basename(temporary),/^rww-[0-9a-f]{8}$/);
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY=evidence;process.env.TEMP=temporary;process.env.TMP=temporary;
const {root,esbuild,playwright,browserLaunchOptions}=require('./test-env.cjs');
const canaries=new Set(),makeKey=()=>{const key='UMQA-'+crypto.randomBytes(12).toString('hex').toUpperCase()+'-'+crypto.randomBytes(4).toString('hex').toUpperCase();canaries.add(key);canaries.add(key.toLowerCase());return key;};
const keyA=makeKey(),keyB=makeKey(),keyC=makeKey();
const safe=value=>{let text=String(value);for(const key of canaries)text=text.split(key).join('[redacted synthetic key]');return text;};
const report={schema:1,mockedNative:true,realEpicActivationVerified:false,physicalGpuMeasurement:false,checks:[],cases:[],geometry:[],screenshots:[],externalRequests:[],errors:[],secretLeakDetected:false};
const check=(name,details={})=>report.checks.push({name,passed:true,...details});

async function fixtureInvoke(name,args={}){
 window.calls.push({name,argNames:Object.keys(args).sort(),batchId:args.batchId||null,keyCount:Array.isArray(args.keys)?args.keys.length:0});
 if(name==='settings_write'||name==='set_ui_language')return;
 if(name==='epic_redeem_batch_prepare')return new Promise((resolve,reject)=>{if(window.pendingPrepare)throw Error('Duplicate synthetic preparation');const pending={keys:args.keys.slice(),channel:args.onProgress,resolve,reject};window.pendingPrepare=pending;window.prepareHistory.push(pending);});
 if(name==='epic_redeem_batch_activate')return new Promise((resolve,reject)=>{if(window.pendingActivate)throw Error('Duplicate synthetic activation');const pending={batchId:args.batchId,channel:args.onProgress,resolve,reject};window.pendingActivate=pending;window.activateHistory.push(pending);});
 if(name==='epic_redeem_batch_cancel')return true;
 if(name==='epic_redeem_batch_export_rejected'){
  const outcome=window.exportResponses.shift()||{saved:true,count:2};
  if(outcome.pending)return new Promise((resolve,reject)=>{if(window.pendingExport)throw Error('Duplicate synthetic export');window.pendingExport={batchId:args.batchId,resolve,reject};});
  if(outcome.error)throw Error(outcome.error);
  return outcome;
 }
 if(name==='epic_redeem_batch_discard'){
  if(window.holdDiscard)return new Promise(resolve=>{window.pendingDiscard=resolve;});
  return;
 }
 if(name==='epic_open_redeem')return new Promise((resolve,reject)=>{window.pendingOfficial={channel:args.onResult,resolve,reject};});
 window.unexpected.push(name);throw Error('Unexpected wizard fixture IPC');
}
const core='export class Channel{constructor(callback){this.onmessage=callback;}}\nexport const invoke='+fixtureInvoke.toString()+';';
const entry=`import React from'react';import{createRoot}from'react-dom/client';
import{RedeemView}from'./src/RedeemView';import{AccountMenu}from'./src/AccountMenu';
import{setLanguagePreference,tr}from'./src/i18n';import{initializeTheme}from'./src/theme';
initializeTheme();setLanguagePreference(window.fixture.locale);window.translate=tr;
function Harness(){
 const[account,setAccount]=React.useState({signedIn:true,displayName:'Synthetic Account'}),[active,setActive]=React.useState(false);
 window.changeAccount=setAccount;window.setRedemptionActive=setActive;
 return <div className="app-shell"><header className="fixture-header"><AccountMenu account={account} busy={false} onLogin={()=>window.loginCalls++} onLogout={async()=>setAccount({signedIn:false,displayName:null})} onRedeem={()=>setActive(true)} onAbout={()=>{}}/></header><main className="main"><h1>Background library</h1><button id="background-action" onClick={()=>window.navigationCalls++}>Background action</button></main><RedeemView account={account} active={active} onClose={()=>{window.closeCalls++;setActive(false);}} onLibrary={()=>{window.libraryCalls++;setActive(false);}} onActivated={()=>window.activatedCalls++}/></div>;
}
const tree=createRoot(document.getElementById('root'));window.unmount=()=>tree.unmount();tree.render(<React.StrictMode><Harness/></React.StrictMode>);`;
const flush=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
const count=(page,name)=>page.evaluate(name=>window.calls.filter(call=>call.name===name).length,name);
const mask=key=>key.split('-').at(-1).slice(-8);
const batchId='11111111-1111-4111-8111-111111111111';
const snapshot=(keys=[keyA,keyB],statuses=keys.map(()=> 'ready'),phase='ready',extra={})=>({batchId,accountName:'Synthetic Account',phase,expiresAt:Math.floor(Date.now()/1000)+300,items:keys.map((key,index)=>({id:'22222222-2222-4222-8222-'+String(index).padStart(12,'0'),index,maskedKey:mask(key),status:statuses[index],title:'Synthetic product '+(index+1),reason:null})),...extra});

async function main(){
 const built=await esbuild.build({stdin:{contents:entry,resolveDir:root,loader:'tsx'},bundle:true,write:false,define:{'process.env.NODE_ENV':'"development"'},loader:{'.svg':'dataurl','.css':'empty'},plugins:[{name:'wizard-fixture',setup(build){build.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'core',namespace:'wizard-fixture'}));build.onLoad({filter:/.*/,namespace:'wizard-fixture'},()=>({contents:core,loader:'js'}));}}]});
 assert(browserLaunchOptions.args.includes('--disable-gpu')&&browserLaunchOptions.args.includes('--disable-gpu-compositing'));
 const css=fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/RedeemView.css'),'utf8')+'\n.fixture-header{padding:12px;display:flex;justify-content:flex-end}.fixture-header .user-area{position:relative}';
 const browser=await playwright.chromium.launch(browserLaunchOptions);
 async function open({locale='ru',theme='dark',viewport={width:1280,height:800},exportResponses=[]}={}){
  const context=await browser.newContext({viewport,locale:'ru-RU',reducedMotion:'reduce'});
  await context.route('**/*',route=>{const url=route.request().url();if(url==='https://um-wizard.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div id="root"></div></body></html>'});if([...canaries].some(key=>url.includes(key)))report.secretLeakDetected=true;report.externalRequests.push({origin:new URL(url).origin});return route.abort();});
  const page=await context.newPage();
  page.on('console',message=>{if([...canaries].some(key=>message.text().includes(key)))report.secretLeakDetected=true;});
  page.on('pageerror',error=>report.errors.push(safe(error.message)));
  await page.goto('https://um-wizard.invalid/');
  await page.evaluate(f=>{window.fixture=f;window.calls=[];window.unexpected=[];window.pendingPrepare=null;window.pendingActivate=null;window.pendingOfficial=null;window.pendingDiscard=null;window.pendingExport=null;window.exportResponses=f.exportResponses;window.prepareHistory=[];window.activateHistory=[];window.libraryCalls=0;window.activatedCalls=0;window.closeCalls=0;window.navigationCalls=0;window.loginCalls=0;localStorage.setItem('unreal-manager.theme.v1',f.theme);localStorage.setItem('unreal-manager.language.v1',f.locale);}, {locale,theme,exportResponses});
  await page.addScriptTag({content:built.outputFiles[0].text});await flush(page);return{page,context};
 }
 async function close(context,page){
  assert.deepEqual(await page.evaluate(()=>window.unexpected),[]);
  const privateState=await page.evaluate(keys=>{const values=JSON.stringify([Object.entries(localStorage),Object.entries(sessionStorage),window.calls]);return keys.every(key=>!values.includes(key)&&!location.href.includes(key));},[...canaries]);
  assert(privateState,'A key escaped into storage/URL/fixture recorder');
  report.cases.push({calls:await page.evaluate(()=>window.calls),keysAbsentFromStorageUrlAndRecorder:privateState});await context.close();
 }
 const wizard=page=>page.locator('.redemption-wizard');
 const closeButton=page=>wizard(page).locator(':scope > .modal-head button.icon');
 const methods=page=>page.locator('.redemption-methods');
 async function openWizard(page){await page.locator('.user-trigger').click();await page.getByRole('menuitem',{name:await page.evaluate(()=>window.translate('Активировать ключ')),exact:true}).click();await wizard(page).waitFor();await flush(page);}
 async function choose(page,mode){await methods(page).locator('button[data-mode="'+mode+'"]').click();await flush(page);}
 async function fill(page,keys=[keyA,keyB]){await page.locator('#redemption-keys').fill(keys.join('\n'));}
 async function paste(page,text){
  const prevented=await page.locator('#redemption-keys').evaluate((field,text)=>{
   field.focus();field.setSelectionRange(0,field.value.length);
   const clipboard=new DataTransfer();clipboard.setData('text/plain',text);
   const event=new ClipboardEvent('paste',{clipboardData:clipboard,bubbles:true,cancelable:true});
   field.dispatchEvent(event);
   // Synthetic clipboard events have no default insertion. Emulate only an allowed browser edit;
   // this never reads or overwrites the user's system clipboard.
   if(!event.defaultPrevented)document.execCommand('insertText',false,text);
   return event.defaultPrevented;
  },text);await flush(page);return prevented;
 }
 async function prepare(page,keys=[keyA,keyB]){await fill(page,keys);await page.locator('.redemption-wizard button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);}
 async function finishPrepare(page,value=snapshot()){await page.evaluate(value=>{const pending=window.pendingPrepare;window.pendingPrepare=null;pending.keys.fill('');pending.resolve(value);},value);await flush(page);}
 async function emitPrepare(page,value){await page.evaluate(value=>window.pendingPrepare.channel.onmessage(value),value);await flush(page);}
 async function finishActivate(page,value){await page.evaluate(value=>{const pending=window.pendingActivate;window.pendingActivate=null;pending.resolve(value);},value);await flush(page);}
 async function back(page){await page.locator('.redemption-wizard-footer').getByRole('button',{name:await page.evaluate(()=>window.translate('Другой способ')),exact:true}).click();await flush(page);}
 async function confirmation(page){await page.locator('.redemption-wizard-footer button.primary').click();await page.locator('.redemption-inline-confirm').waitFor();await flush(page);return page.locator('.redemption-inline-confirm');}
 const exportButton=page=>page.locator('.redemption-export-bar button');
 const rejectedPreview=(extra={})=>{
  const value=snapshot([keyA,keyB,keyC],['failed','failed','ready'],'ready',{rejectedCount:2,...extra});
  value.items[0].reason='alreadyUsed';value.items[1].reason='alreadyOwned';return value;
 };
 async function prepareRejected(page,extra={}){await choose(page,'batch');await prepare(page,[keyA,keyB,keyC]);await finishPrepare(page,rejectedPreview(extra));await exportButton(page).waitFor();}
 async function exportReceipt(page,result){await page.evaluate(result=>{const pending=window.pendingExport;window.pendingExport=null;pending.resolve(result);},result);await flush(page);}
 async function assertExportMetadata(page,expectedCalls){
  const calls=await page.evaluate(()=>window.calls.filter(row=>row.name==='epic_redeem_batch_export_rejected'));
  assert.equal(calls.length,expectedCalls);for(const call of calls){assert.deepEqual(call.argNames,['batchId']);assert.equal(call.batchId,batchId);assert.equal(call.keyCount,0);}
 }
 async function geometry(page,details){
  const value=await wizard(page).evaluate(dialog=>{
   const box=node=>{if(!node)return null;const r=node.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
   const body=dialog.querySelector('.redemption-wizard-body'),footer=dialog.querySelector('.redemption-wizard-footer'),head=dialog.querySelector('.modal-head');
   return{viewport:{width:innerWidth,height:innerHeight},dialog:box(dialog),head:box(head),body:{...box(body),overflowY:getComputedStyle(body).overflowY,scrollHeight:body.scrollHeight,clientHeight:body.clientHeight},footer:box(footer),close:box(head?.querySelector('button.icon')),horizontalOverflow:dialog.scrollWidth>dialog.clientWidth+1,documentOverflow:document.documentElement.scrollWidth>innerWidth+1};
  });
  report.geometry.push({...details,...value});
  const inside=box=>box&&box.x>=-1&&box.right<=value.viewport.width+1&&box.y>=-1&&box.bottom<=value.viewport.height+1;
  assert(inside(value.dialog)&&inside(value.head)&&inside(value.close)&&inside(value.footer),'Wizard header/actions exceed viewport '+JSON.stringify(details));
  assert(value.body.height>30,'Wizard body collapsed');assert(!value.horizontalOverflow&&!value.documentOverflow,'Wizard horizontal overflow');
  assert(value.head.bottom<=value.body.y+1&&value.body.bottom<=value.footer.y+1,'Wizard fixed sections overlap');
  return value;
 }
 async function screenshot(page,step,theme){
  assert.equal(await page.locator('#redemption-keys').count(),0,'Raw key field must be absent before visual capture');
  const privateDom=await page.evaluate(keys=>{
   const content=document.documentElement.outerHTML+'\n'+[...document.querySelectorAll('input,textarea')].map(node=>node.value).join('\n');
   return keys.every(key=>!content.includes(key));
  },[...canaries]);
  assert(privateDom,'A complete synthetic canary remains in DOM before visual capture');
  const file=path.join(evidence,'wizard-'+step+'-ru-'+theme+'-900x600.png');
  await page.screenshot({path:file});
  report.screenshots.push({step,locale:'ru',theme,viewport:{width:900,height:600},path:file,sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'),rawKeyFieldAbsent:true,fullSyntheticKeysAbsentFromDom:true,accountIsSynthetic:true});
 }
 try{
  {
   const{page,context}=await open();assert.equal(await wizard(page).count(),0);await openWizard(page);
   assert.equal(await methods(page).locator('button').count(),3);assert.equal(await page.getByRole('dialog').count(),1);
   assert.equal(await count(page,'epic_redeem_batch_prepare'),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_open_redeem'),0);
   assert(await page.evaluate(()=>document.querySelector('.redemption-wizard').contains(document.activeElement)));
   for(let n=0;n<8;n++){await page.keyboard.press(n%2?'Shift+Tab':'Tab');assert(await page.evaluate(()=>document.querySelector('.redemption-wizard').contains(document.activeElement)));}
   await page.keyboard.press('Escape');await wizard(page).waitFor({state:'detached'});assert(await page.locator('.user-trigger').evaluate(node=>node===document.activeElement));
   assert.equal(await page.evaluate(()=>window.navigationCalls),0);check('Actual account menu opens one master dialog with three side-effect-free methods; Tab stays inside, Escape closes and restores opener focus');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'single');
   const field=page.locator('#redemption-keys');assert.equal(await field.evaluate(node=>node.tagName),'INPUT');assert.equal(await field.getAttribute('type'),'text');assert.equal(await page.locator('.redemption-input-form textarea').count(),0);
   const initialBox=await field.boundingBox();assert(initialBox&&initialBox.height>=30&&initialBox.height<=56,'Single-key input must have one-line height');
   await field.fill('A'.repeat(128));const longBox=await field.boundingBox();assert(longBox&&Math.abs(longBox.height-initialBox.height)<=1,'Long input changed the single-line field height');
   for(const value of [keyA+'\n'+keyB,keyA+'\r\n'+keyB,keyA+'\r'+keyB,keyA+'\n'+keyA]){
    await field.fill(keyC);assert.equal(await paste(page,value),true,'Multiline clipboard input must be rejected before browser insertion');await page.getByRole('alert').waitFor();assert.equal(await field.inputValue(),keyC,'Rejected paste altered the existing input');assert.equal(await count(page,'epic_redeem_batch_prepare'),0);
   }
   const dropped=await field.evaluate((field,text)=>{const data=new DataTransfer();data.setData('text/plain',text);const event=new DragEvent('drop',{dataTransfer:data,bubbles:true,cancelable:true});field.dispatchEvent(event);return event.defaultPrevented;},keyA+'\r\n'+keyB);await flush(page);assert.equal(dropped,true);assert.equal(await field.inputValue(),keyC,'Rejected multiline drop altered the input');
   await field.fill('');await paste(page,keyA);assert.equal(await field.inputValue(),keyA);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await page.locator('.redemption-wizard button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);
   assert.equal(await page.evaluate(()=>window.pendingPrepare.keys.length),1);assert(await page.evaluate(key=>window.pendingPrepare.keys[0]===key,keyA),'Valid pasted key changed before native preparation');await finishPrepare(page,snapshot([keyA]));
   assert.equal(await page.locator('.redemption-table tbody tr').count(),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   check('Single-key mode is a fixed-height text input; multiline paste/drop is rejected before concatenation without RPC, and one valid paste waits for explicit preparation without activation');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');
   await page.locator('input[type=file]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.from('\ufeff'+keyA+'\r\n\r\n'+keyA.toLowerCase()+'\n'+keyB)});
   await page.waitForFunction(()=>document.querySelector('#redemption-keys')?.value.includes('\n'));
   assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await page.locator('.redemption-wizard button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);
   assert.equal(await page.evaluate(()=>window.pendingPrepare.keys.length),2);await finishPrepare(page);
   assert.equal(await page.locator('.redemption-table tbody tr').count(),2);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   check('Batch TXT selection is local, supports BOM/mixed newlines/dedup, and only explicit Check prepares the two unique keys');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');
   await page.evaluate(()=>{window.fileTextCalls=0;File.prototype.text=function(){window.fileTextCalls++;return Promise.resolve('unused');};});
   await page.locator('input[type=file]').setInputFiles({name:'oversized.txt',mimeType:'text/plain',buffer:Buffer.alloc(65537,65)});await page.getByRole('alert').waitFor();
   assert.equal(await page.evaluate(()=>window.fileTextCalls),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);
   const tooMany=Array.from({length:101},makeKey);await fill(page,tooMany);await page.locator('.redemption-wizard button[type=submit]').click();assert.equal(await count(page,'epic_redeem_batch_prepare'),0);
   check('Wizard keeps TXT byte and 100-unique-key limits; oversized files are rejected before File.text and no requests start');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');await prepare(page,[keyA,keyB,keyC]);
   const preview=snapshot([keyA,keyB,keyC],['failed','ready','ready']);preview.items[0].reason='alreadyOwned';await finishPrepare(page,preview);
   const confirm=await confirmation(page);assert.equal(await page.getByRole('dialog').count(),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   const commit=wizard(page).locator('button.primary:visible').filter({hasText:await page.evaluate(()=>window.translate('Активировать готовые ключи'))}).last();assert(await commit.isDisabled());
   await confirm.locator('input[type=checkbox]').check();await commit.click();await page.waitForFunction(()=>!!window.pendingActivate);
   assert.equal(await page.locator('.redemption-table tbody tr').count(),2);assert.equal(await page.locator('.redemption-row-failed').count(),0);
   const done=snapshot([keyB,keyC],['success','success'],'completed');await finishActivate(page,done);
   assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.locator('.redemption-row-success').count(),2);assert.equal(await page.evaluate(()=>window.activatedCalls),1);
   check('Inline confirmation stays inside a single master; explicit account acceptance activates only the two ready rows and success refresh is emitted once');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');await prepare(page);await finishPrepare(page);await confirmation(page);
   await page.keyboard.press('Escape');await wizard(page).waitFor();assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   assert.equal(await page.locator('.redemption-inline-confirm').count(),0);assert.equal(await page.locator('.redemption-row-ready').count(),2);
   check('Escape leaves only inline confirmation and returns to the prepared list without committing or closing the master');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');await prepare(page);
   await emitPrepare(page,snapshot([keyA,keyB],['checking','pending'],'preparing'));
   assert(await closeButton(page).isDisabled());await page.keyboard.press('Escape');assert.equal(await wizard(page).count(),1);assert.equal(await page.evaluate(()=>window.closeCalls),0);
   await page.getByRole('button',{name:await page.evaluate(()=>window.translate('Остановить после текущего')),exact:true}).click();assert.equal(await count(page,'epic_redeem_batch_cancel'),1);
   await finishPrepare(page,snapshot([keyA,keyB],['notAttempted','notAttempted'],'stopped'));
   await closeButton(page).click();await wizard(page).waitFor({state:'detached'});await openWizard(page);
   assert.equal(await methods(page).locator('button').count(),3);assert.equal(await page.locator('.redemption-table').count(),0);
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),snapshot());await flush(page);assert.equal(await page.locator('.redemption-table').count(),0);
   check('Busy X/Escape cannot hide a running operation; explicit stop sends one cancellation, reopening starts clean and late progress cannot restore the old package');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');await fill(page);await closeButton(page).click();await wizard(page).waitFor({state:'detached'});await openWizard(page);await choose(page,'batch');
   assert.equal(await page.locator('#redemption-keys').inputValue(),'');await prepare(page);await finishPrepare(page);
   await page.evaluate(()=>{window.holdDiscard=true;});await back(page);assert.equal(await count(page,'epic_redeem_batch_discard'),1);
   assert.equal(await methods(page).count(),0);await page.evaluate(()=>{window.holdDiscard=false;window.pendingDiscard();window.pendingDiscard=null;});await flush(page);
   await methods(page).waitFor();await choose(page,'single');assert.equal(await page.locator('#redemption-keys').inputValue(),'');
   check('Closing clears private input; changing method waits for native preview discard and cannot carry batch authorization into a new mode');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'batch');await prepare(page);await finishPrepare(page);await confirmation(page);
   await page.evaluate(()=>window.changeAccount({signedIn:true,displayName:'Other Synthetic Account'}));await flush(page);
   assert.equal(await page.locator('.redemption-inline-confirm').count(),0);assert.equal(await page.locator('.redemption-table').count(),0);assert.equal(await count(page,'epic_redeem_batch_discard'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   check('Account changes invalidate the preview and confirmation before any final activation');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await choose(page,'official');assert.equal(await count(page,'epic_redeem_batch_prepare'),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   if(await count(page,'epic_open_redeem')===0)await wizard(page).getByRole('button',{name:await page.evaluate(()=>window.translate('Открыть окно активации')),exact:true}).click();
   await page.waitForFunction(()=>!!window.pendingOfficial);assert.equal(await count(page,'epic_open_redeem'),1);assert(await closeButton(page).isDisabled());await page.keyboard.press('Escape');assert.equal(await wizard(page).count(),1);
   await page.evaluate(()=>{window.pendingOfficial.channel.onmessage({status:'success',title:'Synthetic official product',errorCode:''});window.pendingOfficial.resolve();window.pendingOfficial=null;});await flush(page);
   assert.equal(await page.evaluate(()=>window.activatedCalls),1);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert(!(await closeButton(page).isDisabled()));
   check('Official route uses only the existing Epic-window command, waits for its completion and never forwards wizard keys to batch activation');await close(context,page);
  }
  {
   const{page,context}=await open({exportResponses:[{pending:true}]});await openWizard(page);await prepareRejected(page);
   assert.equal(await page.locator('.redemption-row-failed').count(),2);assert((await exportButton(page).innerText()).includes('(2)'));
   await exportButton(page).evaluate(button=>{button.click();button.click();});await page.waitForFunction(()=>!!window.pendingExport);await flush(page);
   await assertExportMetadata(page,1);assert(await closeButton(page).isDisabled());assert(await exportButton(page).isDisabled());
   assert.equal(await page.getByRole('button',{name:await page.evaluate(()=>window.translate('Остановить после текущего')),exact:true}).count(),0);
   await page.keyboard.press('Escape');assert.equal(await wizard(page).count(),1);assert.equal(await page.evaluate(()=>window.closeCalls),0);
   assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   await exportReceipt(page,{saved:true,count:2});await page.getByRole('status').filter({hasText:'Сохранено отклонённых ключей: 2'}).waitFor();
   assert.equal(await page.evaluate(()=>window.activatedCalls),0);assert(!(await closeButton(page).isDisabled()));
   await closeButton(page).click();await wizard(page).waitFor({state:'detached'});await openWizard(page);assert.equal(await page.locator('.redemption-export-bar').count(),0);
   check('Export requests contain only batchId, duplicate clicks create one save operation, X/Escape wait without a false Stop, and a saved receipt is cleared on reopening');await close(context,page);
  }
  {
   const outcomes=[{saved:false,count:0},{error:'Не удалось сохранить TXT. Выберите другое место и повторите.'},{saved:true,count:101},{saved:true,count:-1},{saved:true,count:1.5},{saved:true,count:0},{saved:'yes',count:2},{saved:true,count:2}];
   const{page,context}=await open({exportResponses:outcomes});await openWizard(page);await prepareRejected(page);
   for(let index=0;index<outcomes.length;index++){
    await exportButton(page).click();await flush(page);await assertExportMetadata(page,index+1);
    const statuses=page.locator('.redemption-export-bar [role=status]');
    if(index<outcomes.length-1)assert.equal(await statuses.count(),0,'Cancelled, failed or malformed export claimed success');
    else{await statuses.waitFor();assert((await statuses.innerText()).includes('2'));}
    if(index>0&&index<outcomes.length-1)await page.getByRole('alert').filter({hasText:'Не удалось сохранить TXT.'}).waitFor();
    assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
    assert.equal(await page.locator('.redemption-wizard-footer button.primary').isDisabled(),false,'File export outcome revoked an otherwise valid ready activation');
   }
   check('Cancelled save, filesystem refusal and malformed receipt counts never claim success; explicit retry reuses the same package without resending keys or activating');await close(context,page);
  }
  {
   const{page,context}=await open();await openWizard(page);await prepareRejected(page);const confirm=await confirmation(page);await confirm.locator('input[type=checkbox]').check();
   await wizard(page).locator('button.primary:visible').last().click();await page.waitForFunction(()=>!!window.pendingActivate);
   assert.equal(await page.locator('.redemption-table tbody tr').count(),1);assert((await exportButton(page).innerText()).includes('(2)'));assert(await exportButton(page).isDisabled());
   await finishActivate(page,snapshot([keyC],['success'],'completed',{rejectedCount:2}));assert((await exportButton(page).innerText()).includes('(2)'));
   await exportButton(page).click();await flush(page);await assertExportMetadata(page,1);await page.locator('.redemption-export-bar [role=status]').waitFor();
   assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.locator('.redemption-row-success').count(),1);assert.equal(await page.evaluate(()=>window.activatedCalls),1);
   await close(context,page);
   {
    const{page,context}=await open({exportResponses:[{saved:true,count:2},{saved:false,count:0}]});await openWizard(page);await prepareRejected(page);const confirm=await confirmation(page);await confirm.locator('input[type=checkbox]').check();await wizard(page).locator('button.primary:visible').last().click();await page.waitForFunction(()=>!!window.pendingActivate);
    await page.evaluate(()=>{const pending=window.pendingActivate;window.pendingActivate=null;pending.reject(Error('Synthetic activation RPC failure'));});await flush(page);
    const originalFailure=page.getByRole('alert').filter({hasText:'Не удалось выполнить операцию.'});await originalFailure.waitFor();assert(await page.locator('.redemption-wizard-footer button.primary').isDisabled());
    for(let index=0;index<2;index++){
     await exportButton(page).click();await flush(page);await assertExportMetadata(page,index+1);assert(await originalFailure.isVisible(),'Saving/cancelling an export cleared the activation failure');assert(await page.locator('.redemption-wizard-footer button.primary').isDisabled(),'Export reauthorized the failed activation');
    }
    assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.evaluate(()=>window.activatedCalls),0);await close(context,page);
   }
   check('Ready-only activation retains original rejected-count metadata for export; successful or cancelled exports preserve a prior activation failure and never reauthorize or replay it');
  }
  {
   {
    const{page,context}=await open({exportResponses:[{pending:true},{pending:true}]});await openWizard(page);const deadline=Math.floor(Date.now()/1000)+120;await prepareRejected(page,{expiresAt:deadline});
    await exportButton(page).click();await page.waitForFunction(()=>!!window.pendingExport);
    await page.evaluate(deadline=>{Date.now=()=>(deadline+1)*1000;},deadline);
    const expired=rejectedPreview({expiresAt:undefined,rejectedCount:2});expired.phase='stopped';expired.items[2].status='notAttempted';expired.items[2].reason='previewExpired';
    await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),expired);await flush(page);
    assert.equal(await exportButton(page).count(),1);assert(await exportButton(page).isDisabled());assert.equal(await page.locator('.redemption-row-ready').count(),0);
    await exportReceipt(page,{saved:true,count:2});await page.locator('.redemption-export-bar [role=status]').waitFor();assert(await page.locator('.redemption-wizard-footer button.primary').isDisabled(),'Saving known rejects restored expired activation authorization');
    // Native export cache has its own lazy 600-second TTL, independent of the expired activation preview.
    // Its actual Instant-based enforcement is checked by native tests; this fixture verifies the error DTO path.
    await exportButton(page).click();await page.waitForFunction(()=>!!window.pendingExport);await page.evaluate(deadline=>{Date.now=()=>(deadline+601)*1000;},deadline);
    await page.evaluate(()=>{const pending=window.pendingExport;window.pendingExport=null;pending.reject(Error('Экспорт недоступен. Проверьте ключи заново.'));});await flush(page);
    await page.getByRole('alert').filter({hasText:'Экспорт недоступен.'}).waitFor();assert.equal(await page.locator('.redemption-export-bar [role=status]').count(),0);
    await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),rejectedPreview());await flush(page);assert.equal(await page.locator('.redemption-row-ready').count(),0);assert(await page.locator('.redemption-wizard-footer button.primary').isDisabled());await page.getByRole('alert').filter({hasText:'Экспорт недоступен.'}).waitFor();
    await assertExportMetadata(page,2);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);await close(context,page);
   }
   {
    const{page,context}=await open({exportResponses:[{pending:true}]});await openWizard(page);await prepareRejected(page);await exportButton(page).click();await page.waitForFunction(()=>!!window.pendingExport);
    await page.evaluate(()=>window.changeAccount({signedIn:true,displayName:'Other Synthetic Account'}));await flush(page);await exportReceipt(page,{saved:true,count:2});
    assert.equal(await page.locator('.redemption-export-bar [role=status]').count(),0);assert.equal(await page.locator('.redemption-table').count(),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
    await close(context,page);
   }
   check('Due preview expiry disables activation while known rejects remain exportable; later native cache-expiry errors and account changes never revive old activation authorization or keys');
  }
  {
   let fixtures=0;
   for(const locale of ['ru','en','de','it','es','fr','zh','ja','ko'])for(const theme of ['dark','light']){
    const viewport={width:900,height:600},details={locale,theme,...viewport};const{page,context}=await open({locale,theme,viewport});await openWizard(page);await geometry(page,{...details,step:'methods'});if(locale==='ru')await screenshot(page,'methods',theme);
    assert.equal(await page.locator('html').getAttribute('data-theme'),theme);if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(await wizard(page).innerText()),'Untranslated wizard method in '+locale);
    await choose(page,'batch');const keys=Array.from({length:30},makeKey);await prepare(page,keys);await finishPrepare(page,snapshot(keys));
    const before=await geometry(page,{...details,step:'review'});if(locale==='ru')await screenshot(page,'review',theme);await page.locator('.redemption-wizard-body').evaluate(node=>{node.scrollTop=node.scrollHeight;});await flush(page);const after=await geometry(page,{...details,step:'review-scrolled'});
    assert(Math.abs(before.head.y-after.head.y)<=1&&Math.abs(before.footer.y-after.footer.y)<=1,'Inner scrolling moved wizard header/footer');
    assert.equal(await page.locator('.redemption-table thead th').count(),3);assert.equal(await page.locator('.redemption-table tbody tr').count(),30);if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(await wizard(page).innerText()),'Untranslated review in '+locale);
    await close(context,page);fixtures++;
   }
   check('All nine languages and both themes keep the 900×600 master, X and footer visible; 30-row results scroll internally without moving fixed actions',{fixtures});
  }
  assert.deepEqual(report.errors,[]);assert.deepEqual(report.externalRequests,[]);assert.equal(report.secretLeakDetected,false);report.passed=true;
 }finally{await browser.close();}
}
main().catch(error=>{report.passed=false;report.failure=safe(error.stack);process.exitCode=1;}).finally(()=>{
 try{if(path.dirname(temporary)!==current||!path.basename(temporary).match(/^rww-[0-9a-f]{8}$/)||fs.lstatSync(temporary).isSymbolicLink())throw Error('Unexpected owned cleanup target');fs.rmSync(temporary,{recursive:true,force:true,maxRetries:10,retryDelay:100});}catch(error){report.cleanupFailure=safe(error.stack);report.passed=false;process.exitCode=1;}
 report.temporaryRemoved=!fs.existsSync(temporary);
 report.scope='Actual AccountMenu/RedeemView/i18n/theme with synthetic IPC/key fixtures only. No real Epic mutation, native app/window, account reuse, key TXT on disk, consumer Windows/DPI or measured physical GPU proof.';
 report.sources=['src/RedeemView.tsx','src/RedeemView.css','src/AccountMenu.tsx','src/ModalFocusScope.tsx','src/redemptionBatch.ts','src/locales/redemption.json'].map(name=>({path:'source/'+name,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(root,name))).digest('hex')}));
 let serialized=JSON.stringify(report,null,2);if([...canaries].some(key=>serialized.includes(key))){report.passed=false;report.secretLeakDetected=true;process.exitCode=1;serialized=safe(JSON.stringify(report,null,2));}
 fs.writeFileSync(path.join(evidence,'wizard-report.json'),serialized,{flag:'wx'});console.log((report.passed?'PASS':'FAIL')+' '+report.checks.length+' redemption wizard checks; evidence '+evidence);if(report.failure)console.error(report.failure);if(report.cleanupFailure)console.error(report.cleanupFailure);
});
