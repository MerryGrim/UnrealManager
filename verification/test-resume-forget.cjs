const fs=require('fs'),assert=require('assert');
const {build}=require('./test-env.cjs').esbuild;
const {chromium}=require('./test-env.cjs').playwright;
const {root,outputDirectory}=require('./test-env.cjs');
(async()=>{
const mock=`export async function invoke(name,args){window.calls.push({name,args});if(name==='engine_resume_tasks')return [{id:'fixture',path:'X:/Fixture/UE_5.8',build:'5.8.3-2',phase:'Скачиваем движок',percent:1,canRecover:false}];}`;
const code=`import React from 'react';import {createRoot} from 'react-dom/client';import {EngineResumeTasks}from './src/EngineResumeTasks';window.calls=[];createRoot(document.getElementById('root')).render(<EngineResumeTasks onResume={()=>{}}/>);`;
const b=await build({stdin:{contents:code,resolveDir:root,loader:'tsx'},bundle:true,write:false,plugins:[{name:'mock',setup(p){p.onResolve({filter:/^@tauri-apps\/api\/core$/},a=>({path:a.path,namespace:'mock'}));p.onLoad({filter:/.*/,namespace:'mock'},()=>({contents:mock,loader:'js'}));}}]});
const browser=await chromium.launch(require('./test-env.cjs').browserLaunchOptions);try{
const page=await browser.newPage({locale:'ru-RU',viewport:{width:1280,height:850}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.setContent('<html><head><meta charset="UTF-8"><style>'+fs.readFileSync(root+'/src/styles.css','utf8')+'</style></head><body><div id="root"></div></body></html>');await page.addScriptTag({content:b.outputFiles[0].text});
await page.getByRole('button',{name:'Убрать задачу из списка',exact:true}).click();const dialog=page.getByRole('dialog',{name:'Убрать задачу из списка?',exact:true});await dialog.getByRole('button',{name:'Отмена',exact:true}).click();assert(!await page.evaluate(()=>window.calls.some(c=>c.name==='engine_resume_forget')));
await page.getByRole('button',{name:'Убрать задачу из списка',exact:true}).click();await page.screenshot({path:outputDirectory+'/beta5-resume-forget.png'});await dialog.getByRole('button',{name:'Убрать из списка',exact:true}).click();await dialog.waitFor({state:'hidden'});assert.deepEqual(await page.evaluate(()=>window.calls.find(c=>c.name==='engine_resume_forget').args),{id:'fixture',expectedPath:'X:/Fixture/UE_5.8'});assert.equal(await page.getByText('Ожидают продолжения',{exact:true}).count(),0);assert.deepEqual(errors,[]);console.log('PASS confirmation cancel keeps task; confirmed removal uses exact id/path and refreshes list');
}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
