const fs=require('fs'),assert=require('assert');
const{build}=require('./test-env.cjs').esbuild;
const{chromium}=require('./test-env.cjs').playwright;
const {root,outputDirectory}=require('./test-env.cjs');
(async()=>{
const mock=`export class Channel{};export async function invoke(name,args){if(name==='engine_pending')return [];if(name==='engine_catalog')return [{appName:'UE_5.8',version:'5.8',build:'5.8.3-2',installed:[{path:'X:/Fixture/UE_5.8',version:'5.8.1',updateAvailable:true}]},{appName:'UE_5.7',version:'5.7',build:'5.7.4-2',installed:[]}];if(name==='engine_prepare')return {id:'plan',appName:'UE_5.8',path:args.path,build:'5.8.3-2',components:Array.from({length:35},(_,i)=>({tag:"component"+i,label:"Дополнительный компонент "+(i+1),bytes:100,selected:true})),downloadBytes:100,freeBytes:1000,requiredBytes:100,conflicts:[]};if(name==='engine_install'){window.channel=args.onProgress;window.channel.onmessage({phase:'Скачиваем движок',done:50,total:100,percent:50,networkBytes:1024,canPause:true,paused:false});return new Promise((resolve,reject)=>{window.failInstall=reject;});}if(name==='engine_pause'){window.channel.onmessage({phase:'Скачиваем движок',done:50,total:100,percent:50,networkBytes:1024,canPause:true,paused:args.paused});}}`;
const code=`import React from'react';import{createRoot}from'react-dom/client';import{EnginesView}from'./src/EnginesView';import{setLanguagePreference}from'./src/i18n';const fixtureRoot=createRoot(document.getElementById('root'));const render=()=>fixtureRoot.render(<EnginesView engines={[{name:'UE',path:'X:/Fixture/UE_5.8',version:'5.8',source:'test'}]} onRefresh={async()=>{}} onProgress={()=>{}}/>);window.setFixtureLocale=locale=>{setLanguagePreference(locale);render();};render();`;
const bundle=await build({stdin:{contents:code,resolveDir:root,loader:'tsx'},bundle:true,write:false,loader:{'.svg':'dataurl'},plugins:[{name:'mock',setup(b){b.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'mock',namespace:'mock'}));b.onLoad({filter:/.*/,namespace:'mock'},()=>({contents:mock,loader:'js'}));}}]});
const browser=await chromium.launch(require('./test-env.cjs').browserLaunchOptions);try{const page=await browser.newPage({locale:'ru-RU',viewport:{width:1280,height:800}});await page.setContent('<html><head><meta charset="UTF-8"><style>'+fs.readFileSync(root+'/src/styles.css','utf8')+'</style></head><body><div id="root"></div></body></html>');await page.addScriptTag({content:bundle.outputFiles[0].text});await page.getByRole('button',{name:'Обновить',exact:true}).click();const dialog=page.getByRole('dialog');await dialog.getByRole('button',{name:'Обновить',exact:true}).click();const pause=dialog.getByRole('button',{name:'Пауза',exact:true}),stop=dialog.getByRole('button',{name:'Остановить загрузку',exact:true});await pause.waitFor();
for(const width of [1280,600]){await page.setViewportSize({width,height:800});const a=await pause.boundingBox(),b=await stop.boundingBox(),meter=await dialog.locator('progress').boundingBox();assert.equal(a.height,44);assert.equal(b.height,44);assert(Math.abs(a.y-b.y)<1,'same row');assert(a.y>meter.y+meter.height,'below meter');assert(a.x+a.width<=b.x+1,'no overlap');assert(b.x+b.width<=width,'inside window');await page.screenshot({path:outputDirectory+'/engine-actions-'+width+'.png'});}
await pause.click();await dialog.getByRole('button',{name:'Продолжить',exact:true}).waitFor();const a=await dialog.getByRole('button',{name:'Продолжить',exact:true}).boundingBox(),b=await stop.boundingBox();assert.equal(a.y,b.y);const errors=[];page.on('pageerror',e=>errors.push(e.message));const headerPage=await browser.newPage({locale:'ru-RU',viewport:{width:600,height:340},deviceScaleFactor:2}),layoutEvidence=[];
headerPage.on('pageerror',e=>errors.push(e.message));
await headerPage.setContent('<html><head><meta charset="UTF-8"><style>'+fs.readFileSync(root+'/src/styles.css','utf8')+'</style></head><body><div id="root"></div></body></html>');await headerPage.addScriptTag({content:bundle.outputFiles[0].text});
await headerPage.getByRole('button',{name:'Доступные',exact:true}).click();await headerPage.getByRole('button',{name:'Установить Unreal Engine 5.7.4',exact:true}).dblclick();
const longDialog=headerPage.getByRole('dialog');await longDialog.locator('input').fill('X:/Fixture/UE_5.7-new');await longDialog.getByRole('button',{name:'Далее',exact:true}).click();await longDialog.locator('.engine-components input').last().waitFor({state:'attached'});assert.equal(await longDialog.locator('input[type=checkbox]').count(),36);
const list=longDialog.locator('.engine-components'),title=longDialog.locator('h2'),closeButton=longDialog.getByRole('button',{name:'Закрыть',exact:true}),footer=longDialog.locator('.wizard-footer');
const rectInside=(r,b)=>r&&r.x>=b.x-1&&r.y>=b.y-1&&r.x+r.width<=b.x+b.width+1&&r.y+r.height<=b.y+b.height+1;
const fixed=async()=>({title:await title.boundingBox(),close:await closeButton.boundingBox(),steps:await longDialog.locator('.wizard-steps').boundingBox(),path:await longDialog.locator('.project-delete-path').boundingBox(),summary:await longDialog.locator('.update-summary').boundingBox(),footer:await footer.boundingBox()});
async function controlTopHit(locator){return locator.evaluate(n=>{const r=n.getBoundingClientRect();return n.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));});}
async function assertSelectionLayout(width,height,position){
 const d=await longDialog.boundingBox(),boxes=await fixed(),viewport={x:0,y:0,width,height},h=await longDialog.locator('.modal-head').boundingBox();
 assert.equal(await title.innerText(),'Установка 5.7.4');
 for(const [name,box]of Object.entries(boxes)){assert(rectInside(box,d)&&rectInside(box,viewport),name+' fully inside dialog and viewport '+width+'x'+height+' '+position);}
 assert(await title.evaluate(n=>{const r=n.getBoundingClientRect();return n.scrollWidth<=n.clientWidth+1&&n.scrollHeight<=n.clientHeight+1&&[r.x+3,r.x+r.width/2,r.right-3].every(x=>n.contains(document.elementFromPoint(x,r.y+r.height/2)));}),'Title is fully drawn and unobscured');
 assert.equal(boxes.close.width,40);assert.equal(boxes.close.height,40);assert(await controlTopHit(closeButton));assert(boxes.title.x+boxes.title.width<=boxes.close.x,'Title and close do not overlap');assert(boxes.steps.y>=h.y+h.height-1&&boxes.path.y>=h.y+h.height-1,'Header does not cover stepper or destination');
 for(const action of await footer.getByRole('button').all()){const r=await action.boundingBox();assert(rectInside(r,viewport)&&await controlTopHit(action),'Footer action remains visible and clickable');}
 const metrics=await list.evaluate(n=>({scrollTop:n.scrollTop,clientHeight:n.clientHeight,scrollHeight:n.scrollHeight,overflow:getComputedStyle(n).overflowY}));assert(metrics.scrollHeight>metrics.clientHeight&&metrics.clientHeight>=48&&['auto','scroll'].includes(metrics.overflow),'Only checklist owns a usable scroll viewport');assert.equal(await longDialog.evaluate(n=>n.scrollTop),0,'Dialog does not scroll around fixed actions');
 const row=position==='first'?list.locator('label').first():list.locator('label').last(),rowRect=await row.boundingBox(),listRect=await list.boundingBox();assert(rectInside(rowRect,listRect),'Requested checklist row is fully visible '+position);assert(await controlTopHit(row.locator('input')),'Visible checklist checkbox is the top hit target');
 return {viewport:{width,height},position,boxes,list:metrics,listRect,rowRect,assertions:{titleVisible:true,closeVisible:true,stepperAndPathVisible:true,summaryVisible:true,footerActionsVisible:true,checklistRowVisible:true,dialogScrollTopZero:true}};
}
for(const [width,height]of [[600,340],[900,600],[1280,800]]){
 await headerPage.setViewportSize({width,height});await list.evaluate(n=>n.scrollTop=0);
 const first=await assertSelectionLayout(width,height,'first');await headerPage.screenshot({path:outputDirectory+'/engine-components-'+width+'x'+height+'-first.png'});
 await list.evaluate(n=>n.scrollTop=n.scrollHeight);const last=await assertSelectionLayout(width,height,'last');
 for(const name of Object.keys(first.boxes)){for(const axis of ['x','y','width','height'])assert(Math.abs(first.boxes[name][axis]-last.boxes[name][axis])<1,'Checklist scrolling keeps '+name+' fixed');}
 await headerPage.screenshot({path:outputDirectory+'/engine-components-'+width+'x'+height+'-last.png'});layoutEvidence.push({...first,screenshot:'engine-components-'+width+'x'+height+'-first.png'},{...last,screenshot:'engine-components-'+width+'x'+height+'-last.png'});
}
// A long translated-title fixture is checked at a height that can contain three header lines.
await headerPage.setViewportSize({width:900,height:600});await title.evaluate(n=>n.textContent='Installation zusätzlicher Komponenten von Unreal Engine und Überprüfung der verfügbaren Dateien');
const longTitle=await title.boundingBox(),closeBox=await closeButton.boundingBox();assert(longTitle.y>=0&&longTitle.y+longTitle.height<=600&&longTitle.x+longTitle.width<=closeBox.x&&await controlTopHit(title)&&await controlTopHit(closeButton));
await headerPage.screenshot({path:outputDirectory+'/engine-components-long-title.png'});
await headerPage.setViewportSize({width:600,height:340});await footer.getByRole('button',{name:'Скачать и установить',exact:true}).click();
await longDialog.getByRole('status').waitFor();assert.equal(await longDialog.locator('.engine-components').count(),0,'Installation step has no checklist');assert.equal(await longDialog.locator('.update-summary').count(),0);assert.equal(await longDialog.evaluate(n=>n.classList.contains('engine-component-step')),false);assert((await longDialog.innerText()).includes('X:/Fixture/UE_5.7-new'));assert.equal(await longDialog.locator('progress').count(),1);
const progressLayout=[];
for(const locale of ['ru','en','de','it','es','fr','zh','ja','ko'])for(const colorScheme of ['dark','light'])for(const [width,height]of [[600,340],[900,600]]){
 await headerPage.setViewportSize({width,height});await headerPage.emulateMedia({colorScheme});
 await headerPage.evaluate(locale=>{window.setFixtureLocale(locale);window.channel.onmessage({phase:'Устанавливаем движок',done:1086,total:250332,percent:0,filesDone:1086,filesTotal:250332,processedBytes:134217728,totalBytes:1073741824,readBytes:7340032,filesPerSecond:47.5,readBytesPerSecond:524288,currentFile:'Engine/Content/'+('LongGermanOrJapaneseComponent_'.repeat(20))+'CurrentAsset.uasset',currentFileStage:'readBoth',currentFileDone:3145728,currentFileTotal:12582912,networkBytes:0,canPause:true,paused:false});},locale);
 const modal=headerPage.getByRole('dialog');await modal.locator('.engine-current-file').waitFor();
 assert.equal(await headerPage.evaluate(()=>window.devicePixelRatio),2);
 assert((await modal.locator('.engine-current-file').getAttribute('title')).endsWith('CurrentAsset.uasset'));
 if(locale!=='ru')assert(!/[А-Яа-яЁё]/.test(await modal.getByRole('status').innerText()),'Progress metrics are translated');
 const bounds=await modal.boundingBox(),viewport={x:0,y:0,width,height};assert(rectInside(bounds,viewport),'Progress dialog within CSS viewport');
 const close=modal.locator('.modal-head>button'),heading=modal.locator('h2');
 for(const control of [close,heading,...await modal.locator('.engine-operation-actions button').all()]){assert(rectInside(await control.boundingBox(),viewport),'Progress heading/action is visible');assert(await controlTopHit(control),'Progress heading/action unobscured');}
 assert.equal(await modal.evaluate(n=>n.scrollWidth<=n.clientWidth+1),true,'Long current path cannot widen dialog');
 const numeric=await modal.locator('.engine-operation>progress').getAttribute('value');assert(Math.abs(Number(numeric)-1086/250332*100)<0.00001);
 assert.equal(await modal.locator('.engine-file-meter').getAttribute('value'),'25','Separate per-file read progress is shown');
 assert(rectInside(await modal.locator('.engine-file-meter').boundingBox(),await modal.locator('.engine-progress-metrics').boundingBox()),'Current-file meter is visible without scrolling even in the short window');
 assert((await modal.locator('.engine-file-work').getAttribute('title')).endsWith('CurrentAsset.uasset'),'Visible file-work row has the full relative path tooltip');
 assert((await modal.getByRole('status').innerText()).includes(['ru','de','it','es','fr'].includes(locale)?'0,4%':'0.4%'));
 if((locale==='ru'&&colorScheme==='dark'&&width===600)||(locale==='de'&&colorScheme==='light'&&width===900)||(locale==='ja'&&colorScheme==='dark'&&width===900))await headerPage.screenshot({path:outputDirectory+'/engine-progress-'+locale+'-'+colorScheme+'-'+width+'.png'});
 progressLayout.push({locale,colorScheme,viewport:{width,height},deviceScaleFactor:2,bounds,close:await close.boundingBox(),actionsVisible:true,currentFileFullTitle:true});
}
fs.writeFileSync(outputDirectory+'/progress-layout-receipt.json',JSON.stringify({schema:1,passed:true,states:progressLayout,scope:'Headless CSS viewports, deviceScaleFactor 2 and OS color-scheme emulation only. Application currently has a dark palette; no app light-theme or physical Windows DPI acceptance claim.'},null,2),{flag:'wx'});
await headerPage.evaluate(()=>window.failInstall('UM_ELEVATION_REQUIRED:'+JSON.stringify({operation:'epicRecord',path:'X:/Fixture/Protected/'+('LongFolder_'.repeat(20))+'Record.item',message:'Не удалось подтвердить разрешение на обновление записи Epic. Повторите операцию.',details:'Access denied (os error 5). '+('Long diagnostic detail '.repeat(30))})));
await longDialog.locator('.engine-permission-error').waitFor();const permissionLayout=[];
for(const locale of ['ru','de','ja'])for(const colorScheme of ['dark','light'])for(const [width,height]of [[600,340],[900,600]]){
 await headerPage.setViewportSize({width,height});await headerPage.emulateMedia({colorScheme});await headerPage.evaluate(locale=>window.setFixtureLocale(locale),locale);
 const error=longDialog.getByRole('alert'),viewport={x:0,y:0,width,height};assert(!/UM_ELEVATION_REQUIRED|"operation"/.test(await error.innerText()));assert((await error.innerText()).includes('X:/Fixture/Protected/'));
 assert((await error.innerText()).includes('Access denied (os error 5)'),'Native cause remains plaintext inside the bounded error panel');
 assert.equal(await longDialog.evaluate(n=>n.scrollWidth<=n.clientWidth+1),true,'Protected paths cannot widen the wizard');
 for(const control of [longDialog.locator('.modal-head>button'),...await longDialog.locator('.wizard-footer button').all()]){const box=await control.boundingBox();assert(rectInside(box,viewport),'Permission error keeps close and retry/back actions visible '+locale+' '+width+' '+JSON.stringify(box));assert(await controlTopHit(control));}
 if(locale==='de'&&width===600&&colorScheme==='dark')await headerPage.screenshot({path:outputDirectory+'/engine-permission-de-short.png'});
 permissionLayout.push({locale,colorScheme,width,height,actionsVisible:true,rawEnvelopeHidden:true});
}
fs.writeFileSync(outputDirectory+'/permission-layout-receipt.json',JSON.stringify({passed:true,states:permissionLayout,scope:'Headless protected-engine-path errors; no native access changes or UAC prompt.'},null,2),{flag:'wx'});
await longDialog.locator('.modal-head>button').click();assert.equal(await headerPage.getByRole('dialog').count(),0);
fs.writeFileSync(outputDirectory+'/components-scroll-receipt.json',JSON.stringify({schema:1,passed:true,states:layoutEvidence,step3ChecklistAbsent:true,step3DestinationAndProgressPresent:true,longTitle900x600Passed:true},null,2),{flag:'wx'});await headerPage.close();
console.log('PASS components-only scroll: first/last checkbox accessible at 600x340, 900x600, 1280x800; title/X, stepper/path/summary/footer stay visible and fixed; installation step removes checklist');
console.log('PASS long translated title and 40px close remain visible without overlap at 900x600');
console.log('PASS actual engine wizard: actions share row below progress, same height, no overlap at 1280/600px, stable resume layout');}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
