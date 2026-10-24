// Pure parser + actual RedeemView with synthetic native IPC only.
// No Epic account, transaction, native app, key file or existing profile is used.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const current=path.resolve(__dirname,'current'),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
assert(parent===current||parent.startsWith(current+path.sep),'Evidence must remain under verification/current');
const stamp=new Date().toISOString().replace(/[:.]/g,'-')+'-'+crypto.randomUUID().slice(0,8);
const evidence=path.join(parent,'redemption-batch-'+stamp),temporary=path.join(current,'rbt-'+crypto.randomUUID().slice(0,8));
assert.equal(path.dirname(temporary),current);fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY=evidence;process.env.TEMP=temporary;process.env.TMP=temporary;
const environment=require('./test-env.cjs'),{root,browserLaunchOptions}=environment;
const canaries=new Set(),makeKey=()=>{const value='UMQA-'+crypto.randomBytes(14).toString('hex').toUpperCase()+'-A1B2';canaries.add(value);canaries.add(value.toLowerCase());return value;};
const keyA=makeKey(),keyB=makeKey(),keyC=makeKey();
const safe=value=>{let text=String(value);for(const key of canaries)text=text.split(key).join('[redacted synthetic key]');return text;};
const report={schema:1,mockedNative:true,realEpicActivationVerified:false,checks:[],cases:[],geometry:[],externalRequests:[],errors:[],secretLeakDetected:false};
const check=(name,details={})=>report.checks.push({name,passed:true,...details});
async function fixtureInvoke(name,args={}){
 const meta={name,argNames:Object.keys(args).sort(),batchId:args.batchId||null,keyCount:Array.isArray(args.keys)?args.keys.length:0,hasProgress:!!args.onProgress};
 window.calls.push(meta);
 if(name==='settings_write'||name==='set_ui_language')return;
 if(name==='epic_redeem_batch_prepare'){
  if(window.pendingPrepare)throw Error('Duplicate synthetic preparation');
  return new Promise((resolve,reject)=>{window.pendingPrepare={keys:args.keys.slice(),channel:args.onProgress,resolve,reject};window.prepareHistory.push(window.pendingPrepare);});
 }
 if(name==='epic_redeem_batch_activate'){
  if(window.pendingActivate)throw Error('Duplicate synthetic activation');
  return new Promise((resolve,reject)=>{window.pendingActivate={batchId:args.batchId,channel:args.onProgress,resolve,reject};window.activateHistory.push(window.pendingActivate);});
 }
 if(name==='epic_redeem_batch_cancel')return true;
 if(name==='epic_redeem_batch_discard')return;
 if(name==='epic_open_redeem')return new Promise((resolve,reject)=>{window.pendingOfficial={channel:args.onResult,resolve,reject};});
 window.unexpected.push(name);throw Error('Unexpected redemption fixture IPC');
}
const core='export class Channel{constructor(callback){this.onmessage=callback;}}\nexport const invoke='+fixtureInvoke.toString()+';';
function fixtureClient(){
 setLanguagePreference(window.fixture.locale||'ru');
 function Harness(){
  const[account,setAccount]=React.useState({signedIn:true,displayName:'DemoUser'}),[active,setActive]=React.useState(true);
  window.changeAccount=setAccount;window.setRedemptionActive=setActive;
  return React.createElement('div',{hidden:!active},React.createElement(RedeemView,{account,active,onLibrary:()=>window.libraryCalls++,onActivated:()=>window.activatedCalls++}));
 }
 const tree=createRoot(document.getElementById('root'));window.unmount=()=>tree.unmount();tree.render(React.createElement(React.StrictMode,null,React.createElement(Harness)));
}
const entry="import React from'react';import{createRoot}from'react-dom/client';import{RedeemView}from'./src/RedeemView';import{setLanguagePreference}from'./src/i18n';\n"+fixtureClient.toString()+'\nfixtureClient();';
async function main(){
 const {esbuild,playwright}=environment;
 const pureBundle=await esbuild.build({entryPoints:[path.join(root,'src/redemptionBatch.ts')],bundle:true,format:'cjs',platform:'node',write:false});
 const parserModule={exports:{}};new Function('module','exports','require',pureBundle.outputFiles[0].text)(parserModule,parserModule.exports,require);
 const {parseRedemptionBatch:parse,maskRedemptionKey:mask,REDEMPTION_BATCH_LIMITS:limits}=parserModule.exports;
 assert.deepEqual([limits.maxKeys,limits.maxUtf8Bytes,limits.maxPhysicalRows,limits.maxKeyCharacters],[100,65536,256,128]);
 {const p=parse('\ufeff  '+keyA+'  \r\n\r\n'+keyA.toLowerCase()+'\n'+keyB+'\r'+keyC+'\r');assert(p.ok&&p.keys.length===3&&p.physicalRows===5&&p.blankRows===1&&p.duplicateLines[0]===3,'BOM/newline/dedup parsing');assert(p.keys[0].key===keyA&&p.keys[1].key===keyB&&p.keys[2].key===keyC,'Parser must preserve first-key case/order');check('Parser handles BOM, mixed newlines, blanks and case-insensitive duplicates while preserving first-key text/order');}
 {const keys=Array.from({length:100},(_,n)=>'SYNTHETIC-'+n);assert(parse(keys.join('\n')).ok);assert(parse(keys.concat(keys).join('\n')).ok);assert(!parse(keys.concat('SYNTHETIC-EXTRA').join('\n')).ok);assert(parse('A'.repeat(128)).ok);assert(!parse('A'.repeat(129)).ok);assert(parse('A\n'+'\n'.repeat(255)).ok);assert(!parse('A\n'+'\n'.repeat(256)).ok);check('Parser accepts exact key/line/physical-row limits and rejects the next unit');}
 {assert(parse('A'+' '.repeat(65535)).ok);const p=parse('A'+' '.repeat(65536));assert(!p.ok&&p.issues.some(x=>x.code==='inputTooLarge'));const utf8=parse('é'.repeat(32769));assert(!utf8.ok&&utf8.issues[0].code==='inputTooLarge');check('Parser enforces the UTF-8 byte limit before large normalization or line processing');}
 {for(const raw of [keyA+' '+keyB,keyA+'\t'+keyB,keyA+'\0',keyA+'Я',keyA+'!',keyA+'https://invalid.invalid','\u00a0'+keyA,keyA+'\ufeff']){const p=parse(keyB+'\n'+raw);assert(!p.ok&&!('keys'in p),'Malformed input must expose no usable key list');assert(!JSON.stringify(p).includes(keyA)&&!JSON.stringify(p).includes(keyB),'Issue diagnostics must exclude input values');}assert(!parse('\r\n \t\r\n').ok);check('Malformed/internal whitespace/control/non-ASCII input fails without a partial key list or reflected secret diagnostics');}
 {for(const value of ['A','ABCD',keyA,keyB.toLowerCase(),'ABC-DEF']){const masked=mask(value);assert(masked.length>0&&!masked.includes(value),'Mask must not expose an entire key, including a short key');}check('Masking never reveals a complete short or long input key');}
 const built=await esbuild.build({stdin:{contents:entry,resolveDir:root,loader:'tsx'},bundle:true,write:false,define:{'process.env.NODE_ENV':'"development"'},loader:{'.svg':'dataurl','.css':'empty'},plugins:[{name:'redemption-fixture',setup(build){build.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'core',namespace:'redemption-fixture'}));build.onLoad({filter:/.*/,namespace:'redemption-fixture'},()=>({contents:core,loader:'js'}));}}]});
 assert(browserLaunchOptions.args.includes('--disable-gpu')&&browserLaunchOptions.args.includes('--disable-gpu-compositing'));
 const browser=await playwright.chromium.launch(browserLaunchOptions),css=fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/RedeemView.css'),'utf8');
 const messages=JSON.parse(fs.readFileSync(path.join(root,'src/locales/redemption.json'),'utf8')),overrides=JSON.parse(fs.readFileSync(path.join(root,'src/locales/overrides.json'),'utf8'));
 const dictionaries=Object.fromEntries(['en','de','it','es','fr','zh','ja','ko'].map(locale=>[locale,{...JSON.parse(fs.readFileSync(path.join(root,'src/locales/'+locale+'.json'),'utf8')),...overrides[locale],...messages[locale]}]));
 const text=(source,locale)=>{const value=locale==='ru'?source:dictionaries[locale][source];assert.equal(typeof value,'string','Missing redemption translation '+locale);return value;};
 const flush=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 const count=(page,name)=>page.evaluate(name=>window.calls.filter(x=>x.name===name).length,name);
 const batchId='11111111-1111-4111-8111-111111111111';
 const snapshot=(statuses=['ready','ready'],phase='ready',extra={},keys=[keyA,keyB])=>({batchId,accountName:'DemoUser',phase,expiresAt:Math.floor(Date.now()/1000)+300,items:statuses.map((status,index)=>({id:'22222222-2222-4222-8222-'+String(index).padStart(12,'0'),index,maskedKey:mask(keys[index]),status,title:'Fictional product '+(index+1),reason:null})),...extra});
 async function open(fixture={}){
  const context=await browser.newContext({viewport:fixture.viewport||{width:1280,height:800},locale:'ru-RU',reducedMotion:'reduce'});
  await context.route('**/*',route=>{const url=route.request().url();if(url==='https://um-redemption.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"><style>'+css+'</style></head><body><div id="root"></div></body></html>'});if([...canaries].some(k=>url.includes(k)))report.secretLeakDetected=true;report.externalRequests.push({origin:new URL(url).origin});return route.abort();});
  const page=await context.newPage();
  page.on('console',message=>{if([...canaries].some(k=>message.text().includes(k)))report.secretLeakDetected=true;});
  page.on('pageerror',error=>{if([...canaries].some(k=>error.message.includes(k)))report.secretLeakDetected=true;report.errors.push(safe(error.message));});
  await page.goto('https://um-redemption.invalid/');
  await page.evaluate(f=>{window.fixture=f;window.calls=[];window.unexpected=[];window.pendingPrepare=null;window.pendingActivate=null;window.pendingOfficial=null;window.prepareHistory=[];window.activateHistory=[];window.libraryCalls=0;window.activatedCalls=0;window.loginEvents=0;window.addEventListener('um-epic-login',()=>window.loginEvents++);},fixture);
  await page.addScriptTag({content:built.outputFiles[0].text});await flush(page);return{page,context};
 }
 async function close(context,page){
  assert.deepEqual(await page.evaluate(()=>window.unexpected),[]);
  const clean=await page.evaluate(keys=>{const storage=JSON.stringify([Object.entries(localStorage),Object.entries(sessionStorage)]),url=location.href,log=JSON.stringify(window.calls);return keys.every(k=>!storage.includes(k)&&!url.includes(k)&&!log.includes(k));},[...canaries]);
  assert(clean,'Key appeared in storage, URL or persisted recorder');
  report.cases.push({calls:await page.evaluate(()=>window.calls),keysAbsentFromStorageUrlAndRecorder:clean});await context.close();
 }
 async function fill(page,keys=[keyA,keyB]){await page.locator('#redemption-keys').fill(keys.join('\n'));}
 async function prepare(page,keys=[keyA,keyB]){await fill(page,keys);await page.locator('.redemption-input-form button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);}
 async function emitPrepare(page,value){await page.evaluate(value=>window.pendingPrepare.channel.onmessage(value),value);await flush(page);}
 async function finishPrepare(page,value=snapshot()){await page.evaluate(value=>{const p=window.pendingPrepare;window.pendingPrepare=null;p.keys.fill('');p.resolve(value);},value);await flush(page);}
 async function emitActivate(page,value){await page.evaluate(value=>window.pendingActivate.channel.onmessage(value),value);await flush(page);}
 async function finishActivate(page,value){await page.evaluate(value=>{const p=window.pendingActivate;window.pendingActivate=null;p.resolve(value);},value);await flush(page);}
 async function noCountdown(page){const slot=page.locator('.redemption-wait-slot');assert.equal(await slot.count(),1);assert.equal(await slot.getAttribute('aria-hidden'),'true');assert.equal((await slot.innerText()).trim(),'');}
 async function confirm(page){
  await page.locator('.redemption-batch .redemption-actions button.primary').click();
  const dialog=page.getByRole('dialog');await dialog.waitFor();return dialog;
 }
 async function activate(page){
  const dialog=await confirm(page);await dialog.getByRole('checkbox').check();await dialog.locator('button.primary').click();await page.waitForFunction(()=>!!window.pendingActivate);
 }
 try{
  {const{page,context}=await open();assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await fill(page);await page.locator('#redemption-keys').press('Enter');assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await page.locator('.redemption-input-form button[type=submit]').evaluate(button=>{button.click();button.click();});await page.waitForFunction(()=>!!window.pendingPrepare);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);const correct=await page.evaluate(keys=>window.pendingPrepare.keys.length===2&&window.pendingPrepare.keys.every((k,n)=>k===keys[n]),[keyA,keyB]);assert(correct,'Preparation input changed');assert.equal(await count(page,'epic_redeem_batch_activate'),0);await finishPrepare(page);assert.equal(await page.locator('#redemption-keys').count(),0);assert.equal(await page.locator('.redemption-table tbody tr').count(),2);assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Paste/newline/Check and StrictMode never activate; duplicate Check creates one preparation and native acceptance clears the raw form');await close(context,page);}
  {const{page,context}=await open();const manyKeys=Array.from({length:30},makeKey);await prepare(page,manyKeys);await emitPrepare(page,snapshot(['checking',...Array(29).fill('pending')],'preparing',{},manyKeys));assert.equal(await page.locator('.redemption-row[aria-current=true]').count(),1);const many=snapshot(Array(30).fill('ready'),'ready',{},manyKeys);await finishPrepare(page,many);assert.equal(await page.locator('.redemption-table thead th').count(),3);assert.equal(await page.locator('.redemption-table tbody tr').count(),30);assert(await page.locator('.redemption-table-scroll').evaluate(el=>el.scrollHeight>el.clientHeight));assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Product preview is a three-column independently scrollable table with a truthful current row before any activation');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);const dialog=await confirm(page);assert.equal(await dialog.locator('button.primary').isDisabled(),true);await dialog.getByRole('checkbox').press('Enter');assert.equal(await count(page,'epic_redeem_batch_activate'),0);await dialog.getByRole('checkbox').check();await dialog.locator('button.primary').evaluate(button=>{button.click();button.click();});await page.waitForFunction(()=>!!window.pendingActivate);assert.equal(await count(page,'epic_redeem_batch_activate'),1);const call=await page.evaluate(()=>window.calls.find(x=>x.name==='epic_redeem_batch_activate'));assert.deepEqual(call.argNames,['batchId','onProgress']);assert.equal(call.batchId,batchId);await finishActivate(page,snapshot(['success','success'],'completed'));assert.equal(await page.evaluate(()=>window.activatedCalls),1);check('Explicit account/product acknowledgment is required and two synchronous final clicks produce one opaque-ID commit');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),snapshot(['checking','pending'],'preparing'));await flush(page);assert.equal(await page.locator('.redemption-row-ready').count(),2);await activate(page);const terminal=snapshot(['unknown','notAttempted'],'stopped');terminal.items[0].reason='unknown';await finishActivate(page,terminal);await page.evaluate(value=>window.activateHistory[0].channel.onmessage(value),snapshot(['activating','ready'],'activating'));await flush(page);assert.equal(await page.locator('.redemption-row-unknown').count(),1);assert.equal(await page.locator('.redemption-row-activating').count(),0);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await count(page,'epic_open_redeem'),0);assert.equal(await page.locator('.redemption-batch .redemption-actions button.primary').isDisabled(),true);check('Authoritative prepare/final results survive late nonterminal Channels; unknown stops without retry or automatic fallback');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);await activate(page);await emitActivate(page,snapshot(['activating','ready'],'activating'));await page.getByRole('button',{name:'Остановить после текущего',exact:true}).click();assert.equal(await count(page,'epic_redeem_batch_cancel'),1);assert.equal(await page.locator('.redemption-row-activating').count(),1);await finishActivate(page,snapshot(['success','notAttempted'],'stopped'));assert.equal(await page.locator('.redemption-row-success').count(),1);assert.equal(await page.locator('.redemption-row-notAttempted').count(),1);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.evaluate(()=>window.activatedCalls),1);check('Cancel requests stop-after-current, retains its result and leaves later rows unattempted without a replay');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await page.evaluate(secret=>{const p=window.pendingPrepare;window.pendingPrepare=null;p.keys.fill('');p.reject(Error(secret));},keyA);await page.getByRole('alert').waitFor();assert(!(await page.getByRole('alert').innerText()).includes(keyA),'Untrusted native error echoed input');assert((await page.locator('#redemption-keys').inputValue()).includes(keyA),'Preflight rejection must retain editable input');assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Rejected preflight retains input but maps reflected raw errors to safe user messages');await close(context,page);}
  {const{page,context}=await open();await prepare(page,[keyA]);await finishPrepare(page,snapshot(['requiresOfficial'],'completed',{},[keyA]));assert.equal(await count(page,'epic_open_redeem'),0);await page.getByRole('button',{name:'Открыть окно активации',exact:true}).click();await page.waitForFunction(()=>!!window.pendingOfficial);const call=await page.evaluate(()=>window.calls.find(x=>x.name==='epic_open_redeem'));assert.deepEqual(call.argNames,['onResult']);assert.equal(await count(page,'epic_redeem_batch_discard'),1);await page.evaluate(()=>{const p=window.pendingOfficial;window.pendingOfficial=null;p.resolve();});await flush(page);assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Official fallback is explicit and independent: discards native preview and passes no key or batch ID to the old window');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await page.evaluate(()=>{const p=window.pendingPrepare;window.pendingPrepare=null;p.keys.fill('');p.reject('UM_EPIC_AUTH_REQUIRED');});await page.getByRole('button',{name:'Войти в Epic',exact:true}).waitFor();assert.equal(await page.evaluate(()=>window.loginEvents),0);assert.equal(await count(page,'epic_open_redeem'),0);await page.getByRole('button',{name:'Войти в Epic',exact:true}).click();assert.equal(await page.evaluate(()=>window.loginEvents),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);check('Authentication requirement offers explicit login/fallback without automatic preparation retry or activation');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);await confirm(page);await page.evaluate(()=>window.changeAccount({signedIn:true,displayName:'OtherDemoUser'}));await flush(page);assert.equal(await page.getByRole('dialog').count(),0);assert.equal(await page.locator('.redemption-table').count(),0);assert.equal(await page.locator('#redemption-keys').inputValue(),'');assert.equal(await count(page,'epic_redeem_batch_discard'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Visible account change invalidates preview/confirmation and clears the input without committing');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await emitPrepare(page,snapshot(['checking','pending'],'preparing'));await page.evaluate(()=>window.setRedemptionActive(false));await flush(page);assert.equal(await count(page,'epic_redeem_batch_cancel'),1);await finishPrepare(page);assert.equal(await count(page,'epic_redeem_batch_discard'),1);await page.evaluate(()=>window.setRedemptionActive(true));await flush(page);assert.equal(await page.locator('.redemption-table').count(),0);assert.equal(await page.locator('#redemption-keys').inputValue(),'');assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Hiding a running batch cancels after current and discards the terminal operation before a clean return');await close(context,page);}
  {const{page,context}=await open();await page.evaluate(()=>{window.fileTextCalls=0;const original=File.prototype.text;File.prototype.text=function(){window.fileTextCalls++;return original.call(this);};});await page.locator('input[type=file]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.alloc(65537,65)});await page.getByRole('alert').waitFor();assert.equal(await page.evaluate(()=>window.fileTextCalls),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('Oversized TXT is rejected from File.size before File.text or any native command');await close(context,page);}
  {const{page,context}=await open();await page.locator('input[type=file]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.from('\ufeff'+keyA+'\r\n\r\n'+keyA.toLowerCase()+'\n'+keyB)});await page.waitForFunction(()=>document.getElementById('redemption-keys').value.length>0);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await page.locator('.redemption-input-form button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);assert.equal(await page.evaluate(()=>window.pendingPrepare.keys.length),2);await finishPrepare(page);assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('TXT import only fills input; explicit Check deduplicates its rows and still does not activate');await close(context,page);}
  {const{page,context}=await open();await page.evaluate(()=>{window.pendingImport=null;File.prototype.text=function(){return new Promise(resolve=>window.pendingImport=resolve);};});await page.locator('input[type=file]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.from(keyA)});await page.waitForFunction(()=>!!window.pendingImport);await fill(page,[keyB]);await page.evaluate(value=>window.pendingImport(value),keyA);await flush(page);assert((await page.locator('#redemption-keys').inputValue())===keyB,'Late import overwrote newer input');await page.locator('input[type=file]').setInputFiles([]);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);check('Late TXT completion and cancelled picker cannot overwrite a newer manual draft or start a request');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);await activate(page);const value=snapshot(['notAttempted','notAttempted'],'stopped');value.items.forEach(x=>x.reason='previewExpired');await finishActivate(page,value);assert.equal(await page.locator('.redemption-row-notAttempted').count(),2);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.evaluate(()=>window.activatedCalls),0);assert.equal(await count(page,'epic_open_redeem'),0);check('An expired native preview is shown as unattempted and cannot silently recheck, commit or switch to fallback');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await emitPrepare(page,snapshot(['pending','pending'],'preparing',{accountName:''}));assert((await page.locator('#redemption-keys').inputValue()).includes(keyA),'An early cancellation ID is not accepted-product confirmation');await page.getByRole('button',{name:'Остановить после текущего',exact:true}).click();assert.equal(await count(page,'epic_redeem_batch_cancel'),1);await finishPrepare(page,snapshot(['notAttempted','notAttempted'],'stopped',{accountName:''}));assert((await page.locator('#redemption-keys').inputValue()).includes(keyA),'Unaccepted cancelled preflight must keep editable input');assert.equal(await count(page,'epic_redeem_batch_activate'),0);check('An early cancellable batch ID does not clear input before account/product acceptance');await close(context,page);}
  {const{page,context}=await open();await fill(page,[keyB]);await page.evaluate(()=>{window.fileTextCalls=0;File.prototype.text=function(){window.fileTextCalls++;return new Promise(resolve=>window.pendingImport=resolve);};});await page.locator('input[type=file]').setInputFiles({name:'fixture.txt',mimeType:'text/plain',buffer:Buffer.from(keyA)});await page.waitForFunction(()=>!!window.pendingImport);await page.locator('input[type=file]').setInputFiles({name:'oversized.txt',mimeType:'text/plain',buffer:Buffer.alloc(65537,65)});await page.getByRole('alert').waitFor();await page.evaluate(value=>window.pendingImport(value),keyA);await flush(page);assert((await page.locator('#redemption-keys').inputValue())===keyB,'An obsolete file replaced the newer rejected-import state');assert.equal(await page.evaluate(()=>window.fileTextCalls),1);assert.equal(await page.getByRole('alert').count(),1);await page.evaluate(()=>window.setRedemptionActive(false));await flush(page);await page.evaluate(()=>window.setRedemptionActive(true));await flush(page);assert.equal(await page.locator('#redemption-keys').inputValue(),'');assert.equal(await page.getByRole('alert').count(),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);check('A new oversized import invalidates an older pending File.text, and leaving the page clears its private draft');await close(context,page);}
  {const{page,context}=await open();await fill(page,[keyA+'!']);await page.locator('.redemption-input-form button[type=submit]').click();await page.getByRole('alert').waitFor();assert(!(await page.getByRole('alert').innerText()).includes(keyA),'Invalid-input UI reflected a key');assert.equal(await count(page,'epic_redeem_batch_prepare'),0);assert.equal(await count(page,'epic_redeem_batch_activate'),0);await fill(page,[keyA]);await flush(page);assert.equal(await count(page,'epic_redeem_batch_prepare'),0);await page.locator('.redemption-input-form button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);await finishPrepare(page,snapshot(['ready'],'ready',{},[keyA]));check('Malformed pasted input is rejected locally without leaking it, and editing alone never sends a request');await close(context,page);}
  {const{page,context}=await open();await prepare(page);await finishPrepare(page);await activate(page);await emitActivate(page,snapshot(['success','ready'],'activating',{waitSeconds:2,waitReason:'pacing'}));await page.getByText('Следующий ключ через 2 с',{exact:true}).waitFor();await emitActivate(page,snapshot(['success','ready'],'activating',{waitSeconds:1,waitReason:'pacing'}));await page.getByText('Следующий ключ через 1 с',{exact:true}).waitFor();await page.getByRole('button',{name:'Остановить после текущего',exact:true}).click();assert.equal(await count(page,'epic_redeem_batch_cancel'),1);await finishActivate(page,snapshot(['success','notAttempted'],'stopped',{waitSeconds:0,waitReason:'pacing'}));await noCountdown(page);assert.equal(await page.locator('.redemption-row-notAttempted').count(),1);assert.equal(await count(page,'epic_redeem_batch_activate'),1);check('Native waitSeconds payload drives the two/one countdown and remains cancellable without a UI timer or second commit');await close(context,page);}
  for(const locale of ['ru','en','de','it','es','fr','zh','ja','ko']){const{page,context}=await open({locale});await prepare(page);await emitPrepare(page,snapshot(['ready','checking'],'preparing',{waitReason:'rateLimited',waitSeconds:60}));await page.getByText(text('Проверка продолжится через {seconds} с',locale).replace('{seconds}','60'),{exact:true}).waitFor();await finishPrepare(page,snapshot(['ready','notAttempted'],'ready',{waitReason:'rateLimited',waitSeconds:0}));await page.getByText(text('Проверена часть списка. Можно активировать готовые ключи; остальные не обработаны.',locale),{exact:true}).waitFor();assert.deepEqual(await page.locator('.redemption-table th').allTextContents(),['Ключ','Продукт','Статус'].map(value=>text(value,locale)));const dialog=await confirm(page);await dialog.getByRole('heading',{name:text('Подтвердите активацию',locale),exact:true}).waitFor();if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(await dialog.innerText()),'Untranslated confirmation in '+locale);assert.equal(await dialog.locator('button.primary').isDisabled(),true);await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});assert.equal(await count(page,'epic_redeem_batch_activate'),0);await close(context,page);}check('Quota countdown, partial preview and mandatory confirmation translate in all nine languages without changing activation intent',{locales:9});
  {
   const{page,context}=await open();const keys=[keyA,keyB,keyC];await prepare(page,keys);
   const value=snapshot(['failed','failed','failed'],'stopped',{},keys);
   value.items.forEach(item=>item.reason='network');
   value.items[0].diagnostic={operation:'evaluate',stage:'productShape',httpStatus:200,productKind:'object',eulaIdsKind:'null',hasErrors:false,success:false,entitlementKind:keyA,value:keyB,requestId:keyC,freeform:{message:keyA}};
   value.items[1].diagnostic={operation:'evaluate',stage:keyA,eulaIdsKind:'null',freeform:keyB};
   value.items[2].diagnostic={operation:keyC,stage:'productShape',eulaIdsKind:'null',value:keyA};
   await finishPrepare(page,value);
   const details=page.locator('.redemption-diagnostic');assert.equal(await details.count(),1);
   await details.getByText('Детали проверки',{exact:true}).click();const rendered=await details.locator('code').innerText();
   for(const allowed of ['evaluate','productShape','HTTP 200','productKind: object','eulaIdsKind: null','hasErrors: false','success: false'])assert(rendered.includes(allowed),'A known diagnostic field was not rendered');
   assert(!['entitlementKind:','requestId','freeform','value:'].some(field=>rendered.includes(field)),'An arbitrary diagnostic field was rendered');
   const leaked=await page.evaluate(secrets=>secrets.some(secret=>document.documentElement.outerHTML.includes(secret)),[...canaries]);
   assert(!leaked,'Diagnostic metadata leaked an arbitrary value');assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_open_redeem'),0);
   check('Closed diagnostic details render product-shape/null metadata while rejecting unknown fields, injected stages/operations and canary values');await close(context,page);
  }
  {
   const{page,context}=await open();const keys=Array.from({length:12},makeKey);await prepare(page,keys);
   const statuses=[...Array(10).fill('ready'),'checking','pending'];
   await emitPrepare(page,snapshot(statuses,'preparing',{waitReason:'rateLimited',waitSeconds:60},keys));
   await page.getByText('Проверка продолжится через 60 с',{exact:true}).waitFor();await page.getByText('Ожидаем повторной проверки',{exact:true}).waitFor();
   assert.equal(await page.locator('.redemption-row-ready').count(),10);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   await page.evaluate(()=>window.preservedReadyRows=[...document.querySelectorAll('.redemption-row-ready')]);
   await emitPrepare(page,snapshot(statuses,'preparing',{waitReason:'rateLimited',waitSeconds:59},keys));await page.getByText('Проверка продолжится через 59 с',{exact:true}).waitFor();
   await emitPrepare(page,snapshot(statuses,'preparing',{waitReason:'rateLimited',waitSeconds:0},keys));await noCountdown(page);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   const partial=[...Array(10).fill('ready'),'notAttempted','notAttempted'];await finishPrepare(page,snapshot(partial,'ready',{waitReason:'rateLimited',waitSeconds:0},keys));
   await page.getByText('Проверена часть списка. Можно активировать готовые ключи; остальные не обработаны.',{exact:true}).waitFor();
   assert(await page.evaluate(()=>window.preservedReadyRows.every((row,index)=>row===document.querySelectorAll('.redemption-row-ready')[index])),'Ready prefix was replaced during a partial result');
   assert.equal(await page.locator('.redemption-row-notAttempted').count(),2);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   const dialog=await confirm(page);assert.equal(await dialog.locator('button.primary').isDisabled(),true);assert((await dialog.innerText()).includes('10'));
   await dialog.getByRole('checkbox').check();await dialog.locator('button.primary').click();await page.waitForFunction(()=>!!window.pendingActivate);
   assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);
   await finishActivate(page,snapshot(Array(10).fill('success'),'completed',{},keys.slice(0,10)));
   assert.equal(await page.locator('.redemption-row-success').count(),10);assert.equal(await page.locator('.redemption-row-notAttempted').count(),0);assert.equal(await page.locator('.redemption-table tbody tr').count(),10);
   check('One preparation RPC preserves ten checked rows across quota countdown and permits only explicit confirmed activation of the ready subset');await close(context,page);
  }
  {
   const{page,context}=await open();await prepare(page);await emitPrepare(page,snapshot(['ready','checking'],'preparing',{waitReason:'rateLimited',waitSeconds:3600}));
   await page.getByText(/^Проверка продолжится через 3\s*600 с$/).waitFor();await page.getByRole('button',{name:'Остановить после текущего',exact:true}).click();
   assert.equal(await count(page,'epic_redeem_batch_cancel'),1);await finishPrepare(page,snapshot(['notAttempted','notAttempted'],'stopped',{waitReason:'rateLimited',waitSeconds:0}));
   assert.equal(await page.locator('.redemption-row-ready').count(),0);await noCountdown(page);assert.equal(await page.locator('.redemption-batch .redemption-actions button.primary').isDisabled(),true);
   assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_open_redeem'),0);
   check('A rate-limit countdown accepts the bounded DTO range and manual cancellation removes ready authorization without automatic work');await close(context,page);
  }
  {
   const{page,context}=await open();const alphaKey='UMQA-'+crypto.randomBytes(10).toString('hex').toUpperCase()+'-SECRETWORD';canaries.add(alphaKey);canaries.add(alphaKey.toLowerCase());
   const keys=[keyA,alphaKey,keyB,keyC];await prepare(page,keys);const value=snapshot(Array(4).fill('failed'),'stopped',{},keys);
   const allowed='errors.com.epicgames.coderedemption.rate_limited',echo='errors.com.epicgames.private.'+mask(alphaKey).replace(/[^a-z0-9]/gi,'').toLowerCase();
   value.items[0].diagnostic={operation:'evaluate',stage:'resultShape',httpStatus:200,serviceCode:allowed};
   value.items[1].diagnostic={operation:'evaluate',stage:'resultShape',serviceCode:echo};
   value.items[2].diagnostic={operation:'evaluate',stage:'resultShape',serviceCode:'errors.com.epicgames.Code-123'};
   value.items[3].diagnostic={operation:'evaluate',stage:'resultShape',serviceCode:keyC};
   await finishPrepare(page,value);const details=page.locator('.redemption-diagnostic');assert.equal(await details.count(),4);
   for(const summary of await details.locator('summary').all())await summary.click();const diagnostics=await details.locator('code').allTextContents();
   assert(diagnostics[0].includes(allowed));assert(!diagnostics.slice(1).some(text=>text.includes('serviceCode:')),'Unsafe or suffix-reflecting service code was displayed');
   assert(!(await page.evaluate(secrets=>secrets.some(secret=>document.documentElement.outerHTML.includes(secret)),[...canaries])),'Diagnostic service code leaked a key');
   check('Canonical service diagnostics display safe codes while suppressing invalid values and alphabetic masked-suffix reflections');await close(context,page);
  }
  {
   const{page,context}=await open();const now=Date.now();await page.clock.install({time:now});const expiresAt=Math.floor(now/1000)+300;
   await prepare(page);const ready=snapshot(['ready','ready'],'ready',{expiresAt});await finishPrepare(page,ready);const dialog=await confirm(page);
   await dialog.getByRole('checkbox').check();const expired=snapshot(['notAttempted','notAttempted'],'stopped',{expiresAt});expired.items.forEach(item=>item.reason='previewExpired');
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),expired);await flush(page);assert.equal(await dialog.isVisible(),true);assert.equal(await page.locator('.redemption-row-ready').count(),2);
   await page.clock.fastForward(300001);assert.equal(await dialog.isVisible(),true,'No UI polling timer may impersonate the native expiry result');
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),expired);await flush(page);await dialog.waitFor({state:'detached'});
   assert.equal(await page.locator('.redemption-row-ready').count(),0);assert.equal(await page.locator('.redemption-batch .redemption-actions button.primary').isDisabled(),true);
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),ready);await flush(page);assert.equal(await page.locator('.redemption-row-ready').count(),0);assert.equal(await page.getByRole('dialog').count(),0);
   assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
   check('Only a due same-batch native expiry after preparation closes confirmation and disables ready activation; early expiry and late ordinary results cannot revive it');await close(context,page);
  }
  {
   const{page,context}=await open(),keys=Array.from({length:12},makeKey);await prepare(page,keys);await finishPrepare(page,snapshot(Array(12).fill('ready'),'ready',{},keys));await activate(page);
   const statuses=[...Array(10).fill('success'),'activating','ready'];await emitActivate(page,snapshot(statuses,'activating',{},keys));
   await page.evaluate(()=>window.preservedSuccessRows=[...document.querySelectorAll('.redemption-row-success')]);
   const waiting=snapshot(statuses,'activating',{waitReason:'rateLimited',waitSeconds:60},keys);waiting.items[10].reason='rateLimited';await emitActivate(page,waiting);
   await page.getByText('Активация продолжится через 60 с',{exact:true}).waitFor();await page.getByText('Ожидаем продолжения активации',{exact:true}).waitFor();
   assert.equal(await page.locator('.redemption-row-success').count(),10);assert.equal(await page.locator('.redemption-row[aria-current=true]').count(),1);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);
   await emitActivate(page,{...waiting,waitSeconds:1});await page.getByText('Активация продолжится через 1 с',{exact:true}).waitFor();
   await emitActivate(page,{...waiting,waitSeconds:0});await noCountdown(page);assert.equal(await page.locator('.redemption-row-activating strong').innerText(),'Активируем…');
   assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await count(page,'epic_open_redeem'),0);
   assert(await page.evaluate(()=>window.preservedSuccessRows.every((row,index)=>row===document.querySelectorAll('.redemption-row-success')[index])),'Quota recovery replaced already-successful rows');
   await finishActivate(page,snapshot(Array(12).fill('success'),'completed',{waitSeconds:0},keys));await noCountdown(page);
   assert.equal(await page.locator('.redemption-row-success').count(),12);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.evaluate(()=>window.activatedCalls),1);
   check('One activation RPC preserves ten successful rows while native throttle countdown and continued results update the current row without a UI retry or fallback');await close(context,page);
  }
  {
   const{page,context}=await open();await prepare(page);await page.getByRole('heading',{name:'Подготавливаем проверку ключей…',exact:true}).waitFor();
   for(const[stage,label]of[['account','Проверяем аккаунт Epic…'],['session','Подготавливаем вход Epic…'],['connection','Подключаемся к Epic…'],['keys','Проверяем ключи…']]){
    await emitPrepare(page,snapshot(['pending','pending'],'preparing',{accountName:'',preparationStage:stage}));await page.getByRole('heading',{name:label,exact:true}).waitFor();
    assert.equal(await page.locator('.redemption-account').count(),0);assert.equal(await page.locator('.redemption-table').count(),0);assert.equal(await page.locator('#redemption-keys').inputValue(),keyA+'\n'+keyB);
   }
   await page.evaluate(()=>{const pending=window.pendingPrepare;window.pendingPrepare=null;pending.keys.fill('');pending.reject(Error('Synthetic preflight rejection'));});await page.getByRole('alert').waitFor();await flush(page);
   assert.equal(await page.locator('#redemption-keys').inputValue(),keyA+'\n'+keyB);assert.equal(await page.locator('.redemption-input-form button[type=submit]').isDisabled(),false);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),snapshot(['pending','pending'],'preparing',{accountName:'',preparationStage:'connection'}));await flush(page);assert.equal(await page.getByRole('heading',{name:'Подключаемся к Epic…',exact:true}).count(),0);
   await page.locator('.redemption-input-form button[type=submit]').click();await page.waitForFunction(()=>!!window.pendingPrepare);await finishPrepare(page);
   assert.equal(await count(page,'epic_redeem_batch_prepare'),2);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_open_redeem'),0);
   check('Preflight stages show truthful account/session/connection/key preparation without clearing input or inventing rows; rejection preserves draft and only explicit retry starts again');await close(context,page);
  }
  {
   const{page,context}=await open(),keys=Array.from({length:12},makeKey);await prepare(page,keys);
   await emitPrepare(page,snapshot([...Array(10).fill('ready'),'pending','pending'],'preparing',{waitReason:'batch',waitSeconds:60},keys));
   await page.getByText('Следующий пакет через 60 с',{exact:true}).waitFor();await page.getByText('Обрабатываем по 10 ключей. После паузы продолжим автоматически.',{exact:true}).waitFor();
   assert.equal(await page.locator('.redemption-row-ready').count(),10);assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);
   await finishPrepare(page,snapshot(Array(12).fill('ready'),'ready',{waitSeconds:0},keys));await activate(page);
   await emitActivate(page,snapshot(Array(12).fill('ready'),'activating',{waitReason:'beforeActivation',waitSeconds:60},keys));
   await page.getByText('Активация начнётся через 60 с',{exact:true}).waitFor();assert.equal(await page.locator('.redemption-row-activating').count(),0);assert.equal(await count(page,'epic_redeem_batch_activate'),1);
   await emitActivate(page,snapshot(Array(12).fill('ready'),'activating',{waitReason:'beforeActivation',waitSeconds:0},keys));await noCountdown(page);assert.equal(await page.locator('.redemption-row-activating').count(),0);assert.equal(await count(page,'epic_redeem_batch_activate'),1);
   await emitActivate(page,snapshot(['activating',...Array(11).fill('ready')],'activating',{},keys));assert.equal(await page.locator('.redemption-row[aria-current=true]').count(),1);
   await finishActivate(page,snapshot(Array(12).fill('success'),'completed',{waitSeconds:0},keys));assert.equal(await page.locator('.redemption-row-success').count(),12);assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await count(page,'epic_open_redeem'),0);
   check('Client group countdown never activates during checking; pre-activation countdown starts only inside the explicitly confirmed single activation RPC');await close(context,page);
  }
  {
   const{page,context}=await open(),keys=Array.from({length:12},makeKey);await prepare(page,keys);
   const checked=snapshot([...Array(10).fill('failed'),'ready','ready'],'ready',{},keys);checked.items.slice(0,10).forEach(item=>{item.reason='alreadyUsed';item.title='';});await finishPrepare(page,checked);
   assert.equal(await page.locator('.redemption-table tbody tr').count(),12);assert.equal(await page.locator('.redemption-row-failed').count(),10);assert.equal(await page.locator('.redemption-row-ready').count(),2);
   await page.evaluate(()=>window.remainingReadyRows=[...document.querySelectorAll('.redemption-row-ready')]);const dialog=await confirm(page);assert((await dialog.innerText()).includes('2'));
   await dialog.getByRole('checkbox').check();await dialog.locator('button.primary').click();await page.waitForFunction(()=>!!window.pendingActivate);await flush(page);
   assert.equal(await page.locator('.redemption-table tbody tr').count(),2);assert.equal(await page.locator('.redemption-row-failed').count(),0);assert.match(await page.locator('.redemption-counts').innerText(),/0\s*\/\s*2/);
   assert(await page.evaluate(()=>window.remainingReadyRows.every((row,index)=>row===document.querySelectorAll('.redemption-row-ready')[index])),'Ready-only selection replaced original eligible row identities');
   const call=await page.evaluate(()=>window.calls.find(value=>value.name==='epic_redeem_batch_activate'));assert.deepEqual(call.argNames,['batchId','onProgress']);assert.equal(call.batchId,batchId);
   await page.evaluate(value=>window.prepareHistory[0].channel.onmessage(value),checked);await flush(page);assert.equal(await page.locator('.redemption-table tbody tr').count(),2);
   const subset={...checked,phase:'activating',items:checked.items.slice(10).map((item,index)=>({...item,index})),waitSeconds:0};await emitActivate(page,subset);
   await finishActivate(page,{...subset,phase:'completed',items:subset.items.map(item=>({...item,status:'success'}))});
   assert.equal(await page.locator('.redemption-row-success').count(),2);assert.equal(await page.locator('.redemption-table tbody tr').count(),2);assert.match(await page.locator('.redemption-counts').innerText(),/2\s*\/\s*2/);
   assert.equal(await count(page,'epic_redeem_batch_activate'),1);assert.equal(await page.evaluate(()=>window.activatedCalls),1);assert.equal(await count(page,'epic_open_redeem'),0);
   check('Rechecking twelve keys with ten already used retains all check results, then explicit activation keeps only two original ready rows and reports counters for that subset');await close(context,page);
  }
  {
   for(const locale of ['ru','de'])for(const viewport of [{width:1280,height:800},{width:900,height:600}]){
    const{page,context}=await open({locale,viewport}),keys=Array.from({length:30},makeKey);await prepare(page,keys);
    const stages=[];
    report.geometry.push({locale,viewport,coordinateSpace:'document CSS pixels',toleranceCssPixels:1,states:stages});
    const capture=async stage=>{
     const geometry=await page.evaluate(()=>{
      const selectors={heading:'.redemption-batch-heading',slot:'.redemption-wait-slot',table:'.redemption-table-scroll',actions:'.redemption-batch > .redemption-actions'};
      const boxes={};for(const[name,selector]of Object.entries(selectors)){
       const element=document.querySelector(selector);if(!element)throw Error('Missing redemption geometry target: '+name);
       const box=element.getBoundingClientRect();boxes[name]={x:box.x+scrollX,y:box.y+scrollY,width:box.width,height:box.height};
      }
      const slot=document.querySelector(selectors.slot),table=document.querySelector(selectors.table);
      window.reservedCountdownSlot??=slot;
      return{boxes,sameSlot:window.reservedCountdownSlot===slot,countdownHidden:slot.getAttribute('aria-hidden')==='true',captionFits:slot.scrollHeight<=slot.clientHeight+1,tableScrolls:table.scrollHeight>table.clientHeight,scrollY};
     });
     const baseline=stages[0];stages.push({stage,...geometry});
     assert(geometry.sameSlot,'Countdown updates replaced the reserved DOM slot');assert(geometry.captionFits,'Countdown caption exceeds its reserved two-line slot');assert(geometry.tableScrolls,'The long-list fixture must exercise the bounded table scroller');
     if(baseline)assert(Math.abs(geometry.scrollY-baseline.scrollY)<=1,'Countdown or ready transition scrolled the page: '+JSON.stringify({baseline:baseline.scrollY,measured:geometry.scrollY}));
     for(const[name,box]of Object.entries(geometry.boxes)){assert(box.width>0&&box.height>0,'Missing physical layout reservation: '+name);if(baseline)for(const field of ['x','y','width','height'])assert(Math.abs(box[field]-baseline.boxes[name][field])<=1,locale+' '+viewport.width+' '+stage+' moved '+name+'.'+field+' '+JSON.stringify({baseline:baseline.boxes[name],measured:box,scrollBefore:baseline.scrollY,scrollNow:geometry.scrollY}));}
    };
    await emitPrepare(page,snapshot(['checking',...Array(29).fill('pending')],'preparing',{},keys));await noCountdown(page);await capture('checking');
    await emitPrepare(page,snapshot(['ready',...Array(29).fill('pending')],'preparing',{waitReason:'pacing',waitSeconds:2},keys));
    await page.getByText(text('Следующий ключ через {seconds} с',locale).replace('{seconds}','2'),{exact:true}).waitFor();await capture('pacing2');
    const quota=['ready','checking',...Array(28).fill('pending')];await emitPrepare(page,snapshot(quota,'preparing',{waitReason:'rateLimited',waitSeconds:60},keys));
    await page.getByText(text('Проверка продолжится через {seconds} с',locale).replace('{seconds}','60'),{exact:true}).waitFor();await capture('quota60');
    await emitPrepare(page,snapshot(quota,'preparing',{waitReason:'rateLimited',waitSeconds:0},keys));await noCountdown(page);await capture('quota0');
    await finishPrepare(page,snapshot(Array(30).fill('ready'),'ready',{waitSeconds:0},keys));await noCountdown(page);await capture('ready');
    assert.deepEqual(stages.map(value=>value.countdownHidden),[true,false,false,true,true]);
    assert.equal(await count(page,'epic_redeem_batch_prepare'),1);assert.equal(await count(page,'epic_redeem_batch_activate'),0);
    await close(context,page);
   }
   check('Reserved countdown and heading keep the same slot, table and action-row geometry through checking, pacing, quota, zero and ready in RU/German at both supported viewports',{viewports:2,locales:2,statesPerFixture:5});
  }
  {
   for(const{locale,viewport}of[{locale:'ru',viewport:{width:900,height:600}},{locale:'de',viewport:{width:650,height:340}}]){
    const{page,context}=await open({locale,viewport}),keys=Array.from({length:12},makeKey);await prepare(page,keys);await finishPrepare(page,snapshot(Array(12).fill('ready'),'ready',{},keys));
    const fixtureGeometry={locale,viewport,scenario:'confirmation portal on transformed scrolled content'};report.geometry.push(fixtureGeometry);
    await page.locator('.content').evaluate(element=>{element.style.height=Math.floor(innerHeight*.8)+'px';element.style.maxHeight=element.style.height;element.style.overflow='auto';element.style.contain='layout paint';element.style.transform='translateZ(0)';element.scrollTop=element.scrollHeight;});
    await flush(page);fixtureGeometry.ancestorBefore=await page.locator('.content').evaluate(element=>({scrollTop:element.scrollTop,clientHeight:element.clientHeight,scrollHeight:element.scrollHeight,transform:getComputedStyle(element).transform,contain:getComputedStyle(element).contain,windowScrollY:scrollY}));
    assert(fixtureGeometry.ancestorBefore.scrollTop>0&&fixtureGeometry.ancestorBefore.scrollHeight>fixtureGeometry.ancestorBefore.clientHeight,'Modal fixture must start inside an actually scrolled content ancestor: '+JSON.stringify(fixtureGeometry.ancestorBefore));
    const trigger=page.locator('.redemption-batch .redemption-actions button.primary');await trigger.evaluate(button=>{window.confirmTriggerElement=button;});
    const dialog=await confirm(page);
    const geometry=await page.evaluate(()=>{
     const backdrop=document.querySelector('.redemption-confirm-backdrop'),dialog=document.querySelector('.redemption-confirm'),body=dialog?.querySelector('.redemption-confirm-body');
     if(!backdrop||!dialog||!body)throw Error('Missing portal confirmation layout');
     const rect=element=>{const r=element.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
     const ancestor=document.querySelector('.content');
     return{portalToBody:backdrop.parentElement===document.body,insideTransformedContent:!!backdrop.closest('.content'),backdrop:rect(backdrop),dialog:rect(dialog),header:rect(dialog.querySelector('#redemption-confirm-title')),actions:rect(dialog.querySelector('.dialog-actions')),body:{...rect(body),clientHeight:body.clientHeight,scrollHeight:body.scrollHeight,overflowY:getComputedStyle(body).overflowY},ancestor:{scrollTop:ancestor.scrollTop,clientHeight:ancestor.clientHeight,scrollHeight:ancestor.scrollHeight,transform:getComputedStyle(ancestor).transform,contain:getComputedStyle(ancestor).contain},viewport:{width:innerWidth,height:innerHeight},scrollY};
    });
    Object.assign(fixtureGeometry,geometry);
    assert(geometry.ancestor.scrollTop>0&&geometry.ancestor.transform!=='none'&&geometry.ancestor.contain.includes('layout')&&geometry.ancestor.contain.includes('paint'),'Confirmation fixture lost the real scrolling/transformed containing block');
    assert(geometry.portalToBody&&!geometry.insideTransformedContent,'Confirmation remains inside the transformed scrolling page');
    assert(Math.abs(geometry.backdrop.x)<=1&&Math.abs(geometry.backdrop.y)<=1&&Math.abs(geometry.backdrop.width-viewport.width)<=1&&Math.abs(geometry.backdrop.height-viewport.height)<=1,'Confirmation backdrop does not cover the viewport');
    for(const name of ['dialog','header','actions']){const box=geometry[name];assert(box.width>0&&box.height>0&&box.x>=0&&box.y>=0&&box.right<=viewport.width+1&&box.bottom<=viewport.height+1,name+' is clipped outside the viewport');}
    for(const name of ['header','actions']){const box=geometry[name];assert(box.y>=geometry.dialog.y-1&&box.bottom<=geometry.dialog.bottom+1,name+' is outside the visible dialog');}
    assert(['auto','scroll'].includes(geometry.body.overflowY),'Long confirmation content lacks its own scroll area');
    if(viewport.height===340)assert(geometry.body.scrollHeight>geometry.body.clientHeight,'Short German modal must exercise real inner overflow');
    await dialog.locator('.redemption-confirm-body').evaluate(element=>{element.scrollTop=element.scrollHeight;});await flush(page);
    const actionsAfter=await dialog.locator('.dialog-actions').boundingBox();assert(actionsAfter&&Math.abs(actionsAfter.y-geometry.actions.y)<=1,'Inner scrolling moved the confirmation actions');
    assert.equal(await dialog.locator('button.primary').isDisabled(),true);await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    assert(await page.evaluate(()=>document.activeElement===window.confirmTriggerElement),'Escape did not restore focus to the actual confirmation trigger');
    assert.equal(await count(page,'epic_redeem_batch_activate'),0);assert.equal(await count(page,'epic_redeem_batch_prepare'),1);
    Object.assign(fixtureGeometry,{escapeFocusRestored:true,activationCalls:0});await close(context,page);
   }
   check('Confirmation covers the viewport above transformed scrolled content, keeps header/actions visible with inner overflow, and Escape returns focus without activation',{fixtures:2});
  }
  assert.deepEqual(report.errors,[]);assert.deepEqual(report.externalRequests,[]);assert.equal(report.secretLeakDetected,false);
  report.passed=true;
 }finally{await browser.close();}
}
main().catch(error=>{report.passed=false;report.failure=safe(error.stack);process.exitCode=1;}).finally(()=>{
 try{if(path.dirname(temporary)!==current||fs.lstatSync(temporary).isSymbolicLink())throw Error('Unexpected transient cleanup target');fs.rmSync(temporary,{recursive:true,force:true,maxRetries:10,retryDelay:100});}catch(error){report.cleanupFailure=safe(error.stack);report.passed=false;process.exitCode=1;}
 report.temporaryRemoved=!fs.existsSync(temporary);
 report.scope='Pure parser and actual React UI with synthetic IPC snapshots only. No real Epic entitlement, redemption, account change, server single-use guarantee, filesystem key import or GPU activity measurement is established.';
 report.sources=['src/RedeemView.tsx','src/RedeemView.css','src/redemptionBatch.ts','src/locales/redemption.json'].map(name=>({path:'source/'+name,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(root,name))).digest('hex')}));
 let serialized=JSON.stringify(report,null,2);if([...canaries].some(k=>serialized.includes(k))){report.passed=false;report.secretLeakDetected=true;process.exitCode=1;serialized=safe(JSON.stringify(report,null,2));}
 fs.writeFileSync(path.join(evidence,'ui-report.json'),serialized,{flag:'wx'});console.log((report.passed?'PASS':'FAIL')+' '+report.checks.length+' redemption batch checks; evidence '+evidence);if(report.failure)console.error(report.failure);if(report.cleanupFailure)console.error(report.cleanupFailure);
});
