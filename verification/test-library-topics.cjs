const fs=require('fs'),assert=require('assert');
const {root,outputDirectory}=require('./test-env.cjs');
const {build}=require('./test-env.cjs').esbuild;
const {chromium}=require('./test-env.cjs').playwright;
(async()=>{
 const item=(id,categories,kind='Asset',engineVersions=['5.8'])=>({id,title:id,categories,kind,engineVersions,source:'Fab',cover:null,versions:[]});
 const items=[...Array.from({length:30},(_,i)=>item('Building '+i,['Buildings & Architecture'])),item('Legacy UI',['UI']),item('Modern UI',['UI & Interfaces'],'Asset',['5.7']),item('UI Plugin',['UI & Interfaces'],'Plugin'),item('Model',['Modeling']),item('Engine 5.10',['Animation'],'Asset',['UE_5.9','UE_5.10']),item('Unknown compatibility',['Animation'],'Asset',[])];
 const mock=`export class Channel{};export async function invoke(name,args){window.calls.push(name);if(name==='fab_cached_library')return {items:${JSON.stringify(items)},cachedAt:1,stale:false};throw Error('Unexpected command '+name);}`;
 const code=`import React from'react';import{createRoot}from'react-dom/client';import{FabLibrary}from'./src/EpicLibrary';window.calls=[];createRoot(document.getElementById('root')).render(<div className="library-view" style={{height:'100vh'}}><FabLibrary account={{signedIn:true,displayName:'Fixture'}} onLogin={()=>{}} onDownload={()=>{}} downloadBusy={false} downloadProgress={null} refreshSignal={0} downloaded={[{id:'vault-ui',assetId:'Legacy UI',title:'Legacy UI'}]} onVaultChanged={async()=>{}}/></div>);`;
 const bundle=await build({stdin:{contents:code,resolveDir:root,loader:'tsx'},bundle:true,write:false,loader:{'.svg':'dataurl'},plugins:[{name:'mock',setup(b){b.onResolve({filter:/^@tauri-apps\/api\/core$/},()=>({path:'mock',namespace:'mock'}));b.onLoad({filter:/.*/,namespace:'mock'},()=>({contents:mock,loader:'js'}));}}]});
 const browser=await chromium.launch(require('./test-env.cjs').browserLaunchOptions);
 try{
  const page=await browser.newPage({locale:'ru-RU',viewport:{width:1280,height:800}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('http://um-library.test/',route=>route.fulfill({contentType:'text/html',body:'<html><head><meta charset="UTF-8"><style>'+fs.readFileSync(root+'/src/styles.css','utf8')+'.library-content{height:100vh}</style></head><body><div id="root"></div></body></html>'}));
  await page.goto('http://um-library.test/');await page.addScriptTag({content:bundle.outputFiles[0].text});
  const topic=page.getByRole('combobox',{name:'Тематика',exact:true});await topic.locator('option').filter({hasText:'Здания и архитектура'}).waitFor({state:'attached'});
  assert.equal(await topic.locator('option').filter({hasText:'Интерфейсы'}).count(),1);
  const sort=page.getByRole('combobox',{name:'Сортировка библиотеки'});
  assert.equal(await sort.locator('option').count(),7);
  assert.equal(await page.locator('.library-filters .page-size').count(),0);
  assert.equal(await page.locator('.library-pagination .page-size').count(),1);
  assert.equal(await page.locator('.fab-tile').first().getAttribute('aria-label'),'Открыть Building 0');
  await page.getByRole('combobox',{name:'Материалов на странице'}).selectOption('24');
  await page.getByRole('button',{name:'Далее',exact:true}).click();
  await sort.selectOption('title-desc');await page.getByText(/Страница 1 из 2/).waitFor();
  assert.equal(await page.locator('.fab-tile').first().getAttribute('aria-label'),'Открыть Unknown compatibility');
  await sort.selectOption('engine-desc');assert.equal(await page.locator('.fab-tile').first().getAttribute('aria-label'),'Открыть Engine 5.10');
  await page.getByRole('button',{name:'Далее',exact:true}).click();assert.equal(await page.locator('.fab-tile').last().getAttribute('aria-label'),'Открыть Unknown compatibility');
  await sort.selectOption('engine-asc');await page.getByText(/Страница 1 из 2/).waitFor();assert.equal(await page.locator('.fab-tile').first().getAttribute('aria-label'),'Открыть Modern UI');
  await sort.selectOption('downloaded');assert.equal(await page.locator('.fab-tile').first().getAttribute('aria-label'),'Открыть Legacy UI');
  await sort.selectOption('not-downloaded');await page.getByRole('button',{name:'Далее',exact:true}).click();assert.equal(await page.locator('.fab-tile').last().getAttribute('aria-label'),'Открыть Legacy UI');
  await sort.selectOption('kind');await page.getByText(/Страница 1 из 2/).waitFor();
  assert.equal(await page.evaluate(()=>localStorage.getItem('um-library-sort')),'kind');
  await sort.selectOption('title-asc');
  for(const width of [1280,900,600]){
   await page.setViewportSize({width,height:700});
   const footer=await page.locator('.library-pagination').boundingBox(),size=await page.getByRole('combobox',{name:'Материалов на странице'}).boundingBox(),next=await page.getByRole('button',{name:'Далее',exact:true}).boundingBox();
   assert(footer&&size&&next&&footer.y+footer.height<=700&&size.y>=footer.y&&size.y+size.height<=700&&next.y+next.height<=700,'bottom controls remain visible at '+width);
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),'no horizontal overflow at '+width);
  }
  await page.setViewportSize({width:1280,height:800});
  await page.getByRole('combobox',{name:'Материалов на странице'}).selectOption('24');await page.getByRole('button',{name:'Далее',exact:true}).click();
  await topic.selectOption({label:'Интерфейсы'});await page.getByText(/Страница 1 из 1/).waitFor();assert.equal(await page.locator('.fab-tile').count(),3);
  await page.getByRole('combobox',{name:'Тип материала'}).selectOption('Asset');assert.equal(await page.locator('.fab-tile').count(),2);
  await page.getByRole('combobox',{name:'Версия Unreal Engine'}).selectOption('5.8');assert.equal(await page.locator('.fab-tile').count(),1);
  await page.getByRole('checkbox',{name:/Показать только скачанное/}).check();assert.equal(await page.locator('.fab-tile').count(),1);
  await page.getByRole('textbox',{name:'Поиск в Fab'}).fill('absent');await page.locator('.empty').waitFor();assert.equal(await page.locator('.fab-tile').count(),0);
  assert.equal(await page.locator('.library-pagination .page-size').count(),1);
  await page.getByRole('textbox',{name:'Поиск в Fab'}).fill('');await page.getByRole('checkbox',{name:/Показать только скачанное/}).uncheck();await page.getByRole('combobox',{name:'Тип материала'}).selectOption('All');await page.getByRole('combobox',{name:'Версия Unreal Engine'}).selectOption('All');
  await topic.selectOption({label:'Здания и архитектура'});assert.equal(await page.locator('.fab-tile').count(),24);await page.getByText(/Найдено: 30/).waitFor();
  await topic.selectOption({label:'Моделирование'});assert.equal(await page.locator('.fab-tile').count(),1);
  assert.deepEqual(await page.evaluate(()=>window.calls),['fab_cached_library']);assert.deepEqual(errors,[]);
  await topic.selectOption({label:'Интерфейсы'});await page.screenshot({path:outputDirectory+'/library-topics.png'});
  await page.setViewportSize({width:900,height:700});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth));
  console.log('PASS cached themes, combined filters, seven local sorts across pages, numeric UE5.10/5.9 and unknown-last, saved sort, page resets, bottom page size retained for empty results, visible footer at1280/900/600, no extra network calls');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
