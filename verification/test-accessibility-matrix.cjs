// Renderer-only DPI/layout and keyboard checks; native commands are synthetic.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {root,outputDirectory,esbuild,playwright,browserLaunchOptions}=require('./test-env.cjs');
const languages=['ru','en','de','es','fr','it','ja','ko','zh'];
const core=`export class Channel{};export async function invoke(name,args={}){
 if(name==='set_ui_language'||name==='settings_write')return;
 if(name==='close_behavior_get')return 'exit';
 if(name==='download_limit_get')return 0;
 if(name==='vault_storage_get')return {vaultPath:'X:/Fixture/Vault',keepEngineBackup:false};
 if(name==='storage_info')return {dataPath:'X:/Fixture/Data',settingsPath:'X:/Fixture/Data/Settings',cachePath:'X:/Fixture/Data/Cache',cacheBytes:0};
 if(name==='project_association_get')return {enabled:false,windowsChoiceRequired:false};
 throw Error('Unexpected native call: '+name);
}`;
(async()=>{
 const report={schema:1,mockedNative:true,osDpiAcceptance:false,checks:[],errors:[]};
 const built=await esbuild.build({stdin:{contents:`import React,{useState}from'react';import{createRoot}from'react-dom/client';import{SettingsView}from'./src/SettingsView';import{ModalFocusScope}from'./src/ModalFocusScope';import{setLanguagePreference,useLanguage,tr}from'./src/i18n';window.chooseLanguage=setLanguagePreference;function Harness(){useLanguage();const[category,setCategory]=useState('interface'),[open,setOpen]=useState(false);const updates={settings:{enabled:false,channel:'stable',configured:true,currentVersion:'0.9.3',mode:'portable',supported:true},checking:false,downloading:false,applying:false,release:null,result:null,error:'',progress:null,check:async()=>{},download:async()=>{},apply:async()=>{},setSettings:async()=>{}};return <><SettingsView category={category} onCategoryChange={setCategory} appUpdates={updates} vaultBusy={false} onVaultChanged={async()=>{}} epicVaultSettings={null}/><button id="open-modal" onClick={()=>setOpen(true)}>{tr('Продолжить')}</button>{open&&<ModalFocusScope onClose={()=>setOpen(false)}><section className="engine-wizard" role="dialog" aria-modal="true" aria-label={tr('Установка движка')}><button data-modal-initial-focus onClick={()=>setOpen(false)}>{tr('Закрыть')}</button><h2>{tr('Установка движка')}</h2><p>{tr('Выберите компоненты')}</p><div className="wizard-footer"><button>{tr('Назад')}</button><button>{tr('Установить')}</button></div></section></ModalFocusScope>}</>};createRoot(document.getElementById('root')).render(<React.StrictMode><Harness/></React.StrictMode>);`,resolveDir:root,loader:'tsx'},bundle:true,write:false,plugins:[{name:'synthetic-native',setup(b){b.onResolve({filter:/^@tauri-apps\/api\/core$/},a=>({path:a.path,namespace:'fixture'}));b.onLoad({filter:/.*/,namespace:'fixture'},()=>({contents:core,loader:'js'}));}}]});
 const browser=await playwright.chromium.launch(browserLaunchOptions);
 try{
  for(const dpr of [1,1.25,1.5,2])for(const width of [900,1280]){
   const page=await browser.newPage({deviceScaleFactor:dpr,viewport:{width,height:width===900?600:800}});
   page.on('pageerror',e=>report.errors.push(e.message));
   await page.route('**/*',r=>r.abort());
   await page.setContent('<html><head><meta charset="UTF-8"><style>'+fs.readFileSync(path.join(root,'src/styles.css'),'utf8')+'</style></head><body><div id="root"></div></body></html>');
   await page.addScriptTag({content:built.outputFiles[0].text});
   for(const lang of languages){
    await page.evaluate(lang=>window.chooseLanguage(lang),lang);
    await page.locator('#close-behavior:not(:disabled)').waitFor();
    const tabs=page.getByRole('tab');assert.equal(await tabs.count(),5);
    await tabs.first().focus();
    for(const key of ['End','Home','ArrowDown','ArrowDown','ArrowDown','ArrowRight','ArrowRight','ArrowUp']){
     await page.keyboard.press(key);
     await page.waitForFunction(()=>{const n=document.activeElement;return n?.getAttribute('role')==='tab'&&n.getAttribute('aria-selected')==='true';});
     const current=await page.evaluate(()=>document.activeElement.id.replace('settings-tab-',''));
     assert.equal(await page.locator('#settings-panel-'+current).isVisible(),true);
     assert.equal(await page.getByRole('tab',{selected:true}).count(),1);
     assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),`${lang}/${dpr}/${width} horizontal overflow`);
     assert(await page.evaluate(()=>{const n=document.activeElement,r=n.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+1&&r.width>0;}),'Focused category is visible');
    }
    await page.locator('#open-modal').click();
    const dialog=page.getByRole('dialog');await dialog.waitFor();
    assert(await dialog.evaluate(n=>n.contains(document.activeElement)),'Initial modal focus');
    const first=await page.evaluate(()=>document.activeElement.textContent);
    await page.keyboard.press('Shift+Tab');
    assert(await dialog.evaluate(n=>n.contains(document.activeElement)),'Reverse focus remains modal');
    await page.keyboard.press('Tab');assert.equal(await page.evaluate(()=>document.activeElement.textContent),first);
    assert(await dialog.evaluate(n=>{const r=n.getBoundingClientRect();return r.left>=0&&r.top>=0&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1;}),'Dialog fits');
    await page.keyboard.press('Escape');await dialog.waitFor({state:'hidden'});
    assert.equal(await page.evaluate(()=>document.activeElement.id),'open-modal');
    await tabs.first().click();
    report.checks.push({lang,dpr,width,passed:true});
   }
   if(dpr===2&&width===900)await page.screenshot({path:path.join(outputDirectory,'accessibility-zh-200.png')});
   await page.close();
  }
  assert.deepEqual(report.errors,[]);report.passed=true;
 }finally{await browser.close();fs.writeFileSync(path.join(outputDirectory,'accessibility-matrix-report.json'),JSON.stringify(report,null,2));}
 console.log(`PASS ${report.checks.length} locale/DPR/viewport combinations; category keys, modal focus, Escape and focus return. OS DPI remains separate.`);
})().catch(e=>{console.error(e);process.exitCode=1;});
