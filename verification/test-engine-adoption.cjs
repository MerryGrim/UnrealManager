const fs = require('node:fs');
const assert = require('node:assert/strict');
const { root, outputDirectory, esbuild, playwright, browserLaunchOptions } = require('./test-env.cjs');

(async () => {
  const mock = `
    export class Channel {}
    export async function invoke(name,args){
      window.calls.push({name,args});
      if(name==='engine_catalog')return [
        {appName:'UE_5.8',version:'5.8',build:'5.8.3-58210709',channel:'stable',installed:[{path:'X:/Existing/UE_5.8',version:'5.8.3',build:'5.8.3-58210709',updateAvailable:false}]},
        {appName:'UE_5.9',version:'5.9',build:'5.9.0-12345',channel:'preview',installed:[]},
        {appName:'UE_5.7',version:'5.7',build:'5.7.0-12345',channel:'unknown',installed:[]}
      ];
      if(name==='engine_pending'||name==='engine_resume_tasks')return [];
      if(name==='choose_project_location')return 'X:/Existing/UE_5.8';
      if(name==='engine_resolve_location')return args.path;
      if(name==='engine_adopt_location')return {path:args.path,version:'5.8'};
      if(name==='engine_prepare')return {id:'adopt',appName:args.appName,build:'5.8.3-58210709',path:args.path,update:true,verify:true,components:[{tag:'templates',label:'Шаблоны проектов',bytes:100,selected:args.selected?.includes('templates')??true}],downloadBytes:100,requiredBytes:100,freeBytes:100000,conflicts:[],changed:0,added:1,removed:0};
      if(name==='engine_install'){
        args.onProgress.onmessage({phase:'Проверяем установленные файлы',done:1,total:4,percent:25,networkBytes:0,filesDone:1,filesTotal:4});
        return new Promise(resolve=>window.completeInstall=()=>resolve('Движок установлен.'));
      }
    }
  `;
  const bundle = await esbuild.build({ stdin: { resolveDir: root, loader: 'tsx', contents: `
    import React from 'react';import {createRoot} from 'react-dom/client';
    import {EnginesView,isPreviewRelease} from './src/EnginesView';import {PageActionsTarget} from './src/PageActions';
    window.calls=[];window.classify=isPreviewRelease;
    function Test(){const [target,setTarget]=React.useState(null);return <PageActionsTarget.Provider value={target}><main className="main workspace-main" style={{height:'100vh',gridTemplateRows:'auto 1fr'}}><header className="workspace-heading"><h1>Unreal Engine</h1><div className="page-actions" ref={setTarget}/></header><EnginesView engines={[{name:'Unreal Engine 5.8',version:'5.8',path:'X:/Existing/UE_5.8',source:'scan'}]} onRefresh={async()=>{window.refreshes=(window.refreshes||0)+1;}} onProgress={()=>{}}/></main></PageActionsTarget.Provider>;}createRoot(document.getElementById('root')).render(<Test/>);
  ` }, bundle: true, write: false, loader: { '.svg': 'dataurl' }, plugins: [{ name: 'native-mock', setup(b) {
    b.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'native',namespace:'native'}));
    b.onLoad({filter:/.*/,namespace:'native'},()=>({contents:mock,loader:'js'}));
  } }] });
  const browser = await playwright.chromium.launch(browserLaunchOptions);
  try {
    const page = await browser.newPage({locale:'ru-RU',viewport:{width:1280,height:850}});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.setContent('<html lang="ru"><head><meta charset="utf-8"><style>'+fs.readFileSync(root+'/src/styles.css','utf8')+'</style></head><body><div id="root"></div></body></html>');
    await page.addScriptTag({content:bundle.outputFiles[0].text});
    await page.getByRole('button',{name:'Доступные',exact:true}).click();
    const stable=page.getByRole('button',{name:'Установить Unreal Engine 5.8.3',exact:true});
    const preview=page.getByRole('button',{name:'Установить Unreal Engine 5.9.0',exact:true});
    await stable.waitFor();assert.equal(await preview.count(),0);assert.equal(await page.getByText('Канал не подтверждён',{exact:true}).count(),1,'Unknown channel remains visible and explicitly unconfirmed');
    await page.getByRole('button',{name:'Превью-версии',exact:true}).click();await preview.waitFor();assert.equal(await stable.count(),0);
    await page.screenshot({path:outputDirectory+'/engine-preview-channel.png'});
    await page.getByRole('button',{name:'Основные релизы',exact:true}).click();await stable.dblclick();
    await page.getByRole('button',{name:'Выбрать',exact:true}).click();
    assert.equal(await page.locator('.engine-path input').inputValue(),'X:/Existing/UE_5.8','Selecting an engine root must not append another UE folder');
    await page.getByRole('button',{name:'Далее',exact:true}).click();
    await page.getByRole('dialog',{name:'Проверка Unreal Engine',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='engine_install').length),0,'Prepare is read-only');
    await page.getByRole('checkbox',{name:'Шаблоны проектов'}).uncheck();
    await page.getByRole('button',{name:'Проверить и доустановить',exact:true}).waitFor();
    await page.screenshot({path:outputDirectory+'/engine-existing-verification.png'});
    await page.getByRole('button',{name:'Закрыть',exact:true}).click();
    await page.getByRole('button',{name:'Добавить установленный',exact:true}).click();
    await page.getByRole('button',{name:'Проверить файлы',exact:true}).waitFor();
    const adoption=await page.evaluate(()=>window.calls.filter(c=>c.name==='engine_adopt_location'));
    assert.equal(adoption.length,1);assert.deepEqual(adoption[0].args,{path:'X:/Existing/UE_5.8'});
    await page.getByRole('button',{name:'Проверить файлы',exact:true}).click();
    await page.getByRole('dialog',{name:'Проверка Unreal Engine',exact:true}).waitFor();
    await page.getByRole('button',{name:'Проверить и доустановить',exact:true}).dblclick();
    await page.waitForFunction(()=>!!window.completeInstall);
    assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='engine_install').length),1);
    assert.match(await page.getByRole('dialog').innerText(),/25%/);
    await page.evaluate(()=>window.completeInstall());await page.getByRole('button',{name:'Готово',exact:true}).click();
    for(const theme of ['dark','light']) {
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      await page.setViewportSize({width:900,height:700});
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
      await page.screenshot({path:outputDirectory+'/engine-adoption-'+theme+'-900.png'});
    }
    const cases=await page.evaluate(()=>[
      window.classify({appName:'UE_5.9',build:'5.9.0-42',channel:'preview'}),
      window.classify({appName:'UE_5.8',build:'5.8.0-42',channel:'stable'}),
      window.classify({appName:'UE_5.0EA',build:'5.0.0-42'}),
      window.classify({appName:'UE_5.9',build:'5.9.0-Preview-42'})
    ]);assert.deepEqual(cases,[true,false,true,true]);assert.deepEqual(errors,[]);
    console.log('PASS separate stable/preview catalogues, explicit metadata channel, existing-root picker, read-only prepare, components, offline adoption, explicit verification, one install, progress, light/dark 900px layout');
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
