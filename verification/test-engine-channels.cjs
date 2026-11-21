const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const {root, outputDirectory, esbuild, playwright, browserLaunchOptions} = require('./test-env.cjs');

// Synthetic public catalogue responses only. No native app, account or cache is changed.
const entry = (appName, build, channel) => ({appName, version:appName.replace('UE_', ''), build, ...(channel === undefined ? {} : {channel}), installed:[]});
const actualPreviewBuild = '5.9.0-60738873+++UE5+Release-5.9-Windows';
const stable = entry('UE_5.8', '5.8.3-58210709', 'stable');
const preview = entry('UE_5.9', actualPreviewBuild, 'preview');
const unknown = entry('UE_5.7', '5.7.0-12345', 'unknown');

(async () => {
  const bundle = await esbuild.build({stdin:{resolveDir:root, loader:'tsx', contents:`
    import React from 'react';import {createRoot} from 'react-dom/client';
    import {EnginesView,engineReleaseChannel} from './src/EnginesView';import {PageActionsTarget} from './src/PageActions';
    window.classifyChannel=engineReleaseChannel;
    function Test(){const [target,setTarget]=React.useState(null);return <PageActionsTarget.Provider value={target}><main className="main workspace-main" style={{height:'100vh',gridTemplateRows:'auto 1fr'}}><header className="workspace-heading"><h1>Unreal Engine</h1><div className="page-actions" ref={setTarget}/></header><EnginesView engines={[]} onRefresh={async()=>{window.refreshes++;}} onProgress={()=>{}}/></main></PageActionsTarget.Provider>}
    createRoot(document.getElementById('root')).render(<Test/>);
  `}, bundle:true, write:false, loader:{'.svg':'dataurl'}, plugins:[{name:'native-mock', setup(build){
    build.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'native',namespace:'native'}));
    build.onLoad({filter:/.*/,namespace:'native'},()=>({loader:'js',contents:`
      export class Channel {}
      export async function invoke(name,args){
        window.calls.push({name,args});
        if(name==='engine_resume_tasks'||name==='engine_pending')return [];
        if(name==='engine_catalog'){
          if(args.cachedOnly)return window.cachedCatalog;
          return new Promise((resolve,reject)=>window.catalogRequests.push({resolve,reject}));
        }
        throw new Error('Unexpected native command: '+name);
      }
    `}));
  }}]});
  const browser = await playwright.chromium.launch(browserLaunchOptions);
  const checks = [], errors = [];
  const check = name => {checks.push(name);console.log('PASS '+name);};
  async function open(cached) {
    const page = await browser.newPage({locale:'ru-RU',viewport:{width:1280,height:850}});
    page.on('pageerror', error => errors.push(error.message));
    await page.setContent('<html lang="ru"><head><meta charset="utf-8"><style>'+fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'</style></head><body><div id="root"></div></body></html>');
    await page.evaluate(cached=>{window.cachedCatalog=cached;window.calls=[];window.catalogRequests=[];window.refreshes=0;},cached);
    await page.addScriptTag({content:bundle.outputFiles[0].text});
    await page.waitForFunction(()=>window.catalogRequests.length===1);
    await page.getByRole('button',{name:'Доступные',exact:true}).click();
    return page;
  }
  const group = (page, name) => page.getByRole('group',{name:'Канал выпусков движка',exact:true}).getByRole('button',{name,exact:true});
  const tile = (page, version) => page.getByRole('button',{name:'Установить Unreal Engine '+version,exact:true});
  async function finish(page, catalogue, index=0) {
    await page.evaluate(({catalogue,index})=>window.catalogRequests[index].resolve(catalogue),{catalogue,index});
    await page.getByRole('button',{name:'Проверить обновления',exact:true}).waitFor({state:'visible'});
  }
  async function refresh(page, catalogue) {
    const index=await page.evaluate(()=>window.catalogRequests.length);
    await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();
    await page.waitForFunction(length=>window.catalogRequests.length===length+1,index);
    await finish(page,catalogue,index);
  }
  try {
    {
      const legacy=entry('UE_5.6','5.6.2-12345');
      const empty=entry('UE_5.5','5.5.4-12345','');
      const page=await open([stable,preview,unknown,legacy,empty]);
      assert.equal(await group(page,'Основные релизы').getAttribute('aria-pressed'),'true');
      await tile(page,'5.8.3').waitFor();assert.equal(await tile(page,'5.9.0').count(),0);
      assert.equal(await page.locator('.engine-channel-unverified').count(),0);
      check('Confirmed stable catalogue excludes preview, unknown and old records without channel');
      await group(page,'Превью-версии').click();await tile(page,'5.9.0').waitFor();
      assert.equal(await tile(page,'5.8.3').count(),0);assert.equal(await page.locator('.engine-channel-unverified').count(),0);
      check('Explicit preview channel places the exact unmarked UE 5.9 build in Preview');
      await group(page,'Канал не подтверждён').click();
      await tile(page,'5.7.0').waitFor();await tile(page,'5.6.2').waitFor();await tile(page,'5.5.4').waitFor();
      assert.equal(await page.locator('.engine-channel-unverified').count(),3);
      assert.equal(await tile(page,'5.8.3').count(),0);assert.equal(await tile(page,'5.9.0').count(),0);
      check('Unknown, omitted and empty channel values remain installable in their own labelled group');
      await finish(page,[stable,preview,unknown,legacy,empty]);
      await page.screenshot({path:path.join(outputDirectory,'engine-channel-unknown.png'),animations:'disabled'});
      const classified=await page.evaluate(()=>{
        const builds=['Preview2','prerelease','pre+++release','Alpha1','Beta2','RC1','earlyaccess','early-access','EA','Nightly','Experimental','release-candidate'];
        return {markers:builds.map(marker=>window.classifyChannel({appName:'UE_5.9',build:'5.9.0-'+marker})),stableConflict:window.classifyChannel({appName:'UE_5.9',build:'5.9.0-RC1',channel:'stable'}),numeric:window.classifyChannel({appName:'UE_5.9',build:'5.9.0-60738873+++UE5+Release-5.9-Windows'}),zeroPatchStable:window.classifyChannel({appName:'UE_5.9',build:'5.9.0-1',channel:'stable'})};
      });
      assert.deepEqual(classified.markers,Array(12).fill('preview'));assert.equal(classified.stableConflict,'preview');assert.equal(classified.numeric,'unknown');assert.equal(classified.zeroPatchStable,'stable');
      check('Legacy marker fallback matches native evidence without treating .0 or Release branches as channel evidence');
      await page.close();
    }
    {
      const page=await open([entry('UE_5.9',actualPreviewBuild,'unknown'),entry('UE_5.8',stable.build,'unknown')]);
      assert.equal(await group(page,'Канал не подтверждён').getAttribute('aria-pressed'),'true');
      await tile(page,'5.9.0').waitFor();assert.equal(await page.locator('.engine-channel-unverified').count(),2);
      check('An all-unknown cached catalogue opens its nonempty group instead of an empty Main catalogue');
      await finish(page,[preview,unknown]);
      assert.equal(await group(page,'Превью-версии').getAttribute('aria-pressed'),'true');
      await tile(page,'5.9.0').waitFor();assert.equal(await page.locator('.engine-channel-unverified').count(),0);
      await page.screenshot({path:path.join(outputDirectory,'engine-channel-59-preview.png'),animations:'disabled'});
      check('Fresh positive metadata moves cached UE 5.9 to Preview and removes the unconfirmed badge');
      await group(page,'Основные релизы').click();
      await page.getByText('Основные релизы сейчас недоступны.',{exact:true}).waitFor();
      await refresh(page,[stable,preview,unknown]);
      assert.equal(await group(page,'Основные релизы').getAttribute('aria-pressed'),'true');await tile(page,'5.8.3').waitFor();
      await refresh(page,[preview,unknown]);
      assert.equal(await group(page,'Основные релизы').getAttribute('aria-pressed'),'true');
      await page.getByText('Основные релизы сейчас недоступны.',{exact:true}).waitFor();
      check('Refresh preserves an explicit channel selection even if that group later becomes empty');
      await group(page,'Канал не подтверждён').click();await tile(page,'5.7.0').waitFor();
      await refresh(page,[stable,preview]);await page.getByText('Выпусков с неподтверждённым каналом сейчас нет.',{exact:true}).waitFor();
      check('The unknown group has an explicit empty state after evidence is resolved');
      for(const theme of ['dark','light']){
        await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);await page.setViewportSize({width:600,height:800});
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
        for(const name of ['Основные релизы','Превью-версии','Канал не подтверждён']){
          const box=await group(page,name).boundingBox();assert(box&&box.x>=0&&box.x+box.width<=600&&box.height>=40,'Channel control stays in viewport');
        }
        await page.screenshot({path:path.join(outputDirectory,'engine-channels-'+theme+'-600.png'),animations:'disabled'});
      }
      check('Three channel controls fit both themes at 600px without horizontal overflow');await page.close();
    }
    {
      const page=await open([entry('UE_5.9',actualPreviewBuild),unknown]);
      await group(page,'Канал не подтверждён').click();
      await finish(page,[stable,preview,unknown]);
      assert.equal(await group(page,'Канал не подтверждён').getAttribute('aria-pressed'),'true');await tile(page,'5.7.0').waitFor();
      assert.equal(await tile(page,'5.9.0').count(),0);
      check('A user selection made while fresh catalogue metadata is pending is preserved');await page.close();
    }
    {
      const sameVersionStable=entry('UE_5.9','5.9.0-70000000','stable');
      const sameVersionPreview=entry('UE_5.9_Preview2','5.9.0-60738873','preview');
      const page=await open([sameVersionStable,sameVersionPreview]);
      await tile(page,'5.9.0').waitFor();assert.equal(await tile(page,'5.9.0').count(),1);
      await group(page,'Превью-версии').click();await tile(page,'5.9.0').waitFor();assert.equal(await tile(page,'5.9.0').count(),1);
      await page.evaluate(()=>window.catalogRequests[0].reject('Synthetic catalogue offline'));
      await page.getByText('Synthetic catalogue offline',{exact:true}).waitFor();await tile(page,'5.9.0').waitFor();
      check('Stable and preview app identities for the same version coexist and cached selection survives refresh failure');await page.close();
    }
    assert.deepEqual(errors,[]);
    fs.writeFileSync(path.join(outputDirectory,'engine-channels-report.json'),JSON.stringify({passed:true,checks,realEpicOrAccountMutation:false},null,2)+'\n');
    console.log('PASS '+checks.length+' engine channel groups; evidence '+outputDirectory);
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
