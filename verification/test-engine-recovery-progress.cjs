// Native recovery is mocked here; Rust tests cover actual file restoration.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {root,outputDirectory,esbuild,playwright,browserLaunchOptions}=require('./test-env.cjs');

(async()=>{
 const core=`export class Channel{};export async function invoke(name,args={}){
  window.calls.push(name);
  if(name==='set_ui_language')return;
  if(name==='engine_pending')return window.finished?[]:[{path:'X:/Fixture/UE_5.8',build:'5.8.4-2'}];
  if(name==='engine_catalog')return [];
  if(name==='engine_resume_tasks')return window.offerTask?[{id:'other-engine',path:'X:/Fixture/UE_5.7',build:'5.7.4-2',phase:'Скачиваем движок',percent:5}]:[];
  if(name==='engine_resume_prepare')return {id:'other-engine',path:'X:/Fixture/UE_5.7',appName:'UE_5.7',build:'5.7.4-2',update:false,components:[],downloadBytes:1,requiredBytes:1,freeBytes:1000,conflicts:[]};
  if(name==='engine_recover'){
   window.recoveryChannel=args.onProgress;
   return new Promise((resolve,reject)=>{window.finish=()=>{window.finished=true;resolve(null)};window.fail=()=>{window.offerTask=true;reject('Ошибка файловой системы (Восстановление файла): X:/Fixture/UE_5.8/Engine/large.bin: Отказано в доступе. (os error 5)')};});
  }
  throw Error('Unexpected command '+name);
 }`;
 const bundle=await esbuild.build({stdin:{contents:`import React from 'react';import{createRoot}from'react-dom/client';import{EnginesView}from'./src/EnginesView';window.calls=[];window.transfers=[];createRoot(document.getElementById('root')).render(<EnginesView engines={[]} onRefresh={async()=>{}} onProgress={p=>window.transfers.push(p)}/>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,loader:{'.svg':'dataurl'},plugins:[{name:'recovery-native-fixture',setup(b){b.onResolve({filter:/^@tauri-apps\/api\/core$/},a=>({path:a.path,namespace:'native-fixture'}));b.onLoad({filter:/.*/,namespace:'native-fixture'},()=>({contents:core,loader:'js'}));}}]});
 const browser=await playwright.chromium.launch(browserLaunchOptions);
 const report={mockedNative:true,realFileRecoveryVerified:false,checks:[],errors:[]};
 try{
  for(const fail of [false,true]){
   const page=await browser.newPage({locale:'ru-RU',viewport:{width:900,height:650}});
   page.on('pageerror',error=>report.errors.push(error.message));
   await page.route('**/*',route=>route.abort());
   await page.setContent('<html><head><meta charset="utf-8"><style>'+fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'</style></head><body><div id="root"></div></body></html>');
   await page.addScriptTag({content:bundle.outputFiles[0].text});
   await page.getByRole('button',{name:'Восстановить предыдущую версию',exact:true}).click();
   const panel=page.getByRole('region',{name:'Восстанавливаем исходные файлы',exact:true});
   // section with an accessible name is exposed as a region by the browser.
   await panel.waitFor();
   assert.equal(await panel.getAttribute('aria-busy'),'true');
   assert.equal(await panel.getByRole('button',{name:'Закрыть',exact:true}).count(),0);
   await page.evaluate(()=>window.recoveryChannel.onmessage({phase:'Восстанавливаем прежние файлы',done:1,total:4,percent:25,filesDone:1,filesTotal:4,readBytes:2097152,currentFile:'Engine/large.bin',currentFileStage:'readOld',currentFileDone:1048576,currentFileTotal:4194304}));
   await panel.getByText('Engine/large.bin',{exact:false}).waitFor();
   assert.equal(await panel.getByRole('progressbar',{name:'Прогресс текущего файла',exact:true}).getAttribute('value'),'25');
   assert.match(await panel.innerText(),/Прочитано/);
   assert.match(await panel.innerText(),/1\s*\/\s*4/);
   const transfer=await page.evaluate(()=>window.transfers.at(-1));
   assert.equal(transfer.currentFile,'Engine/large.bin');assert.equal(transfer.canPause,false);assert.equal(transfer.bytesPerSecond,0);
   await page.screenshot({path:path.join(outputDirectory,fail?'engine-recovery-before-error.png':'engine-recovery-progress.png')});
   await page.evaluate(fail=>fail?window.fail():window.finish(),fail);
   await page.waitForFunction(()=>document.querySelector('.engine-recovery-progress')?.getAttribute('aria-busy')==='false');
   if(fail){await panel.getByRole('alert').waitFor();assert.match(await panel.getByRole('alert').innerText(),/large\.bin.*os error 5/);}
   else await panel.getByText('Изменения отменены',{exact:true}).waitFor();
   assert.equal(await page.evaluate(()=>window.transfers.at(-1)),null);
   assert.equal(await page.evaluate(()=>window.calls.filter(c=>c==='engine_recover').length),1);
   assert.equal(await page.evaluate(()=>window.calls.includes('engine_cancel')),false);
   if(fail){
    // Starting a different saved task must retire the old recovery panel.
    const task=page.locator('.engine-resume-task').filter({hasText:'X:/Fixture/UE_5.7'});
    await task.getByRole('button',{name:'Продолжить установку',exact:true}).click();
    await panel.waitFor({state:'hidden'});await page.getByRole('dialog').waitFor();
    assert.equal(await page.evaluate(()=>window.calls.filter(c=>c==='engine_resume_prepare').length),1);
   }else{await panel.getByRole('button',{name:'Закрыть',exact:true}).click();await panel.waitFor({state:'hidden'});}
   report.checks.push(fail?'recovery failure retains exact file/error, releases busy state and dismisses':'recovery reports per-file/read bytes to panel and current transfers, completes and dismisses');
   await page.close();
  }
  assert.deepEqual(report.errors,[]);report.passed=true;
 }finally{await browser.close();fs.writeFileSync(path.join(outputDirectory,'engine-recovery-progress-report.json'),JSON.stringify(report,null,2));}
 console.log('PASS recovery progress, current transfer, blocked dismissal, success/error cleanup');
})().catch(error=>{console.error(error);process.exitCode=1;});
