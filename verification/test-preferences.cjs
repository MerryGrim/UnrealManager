const fs=require('fs'),assert=require('assert'),vm=require('vm');
const {buildSync}=require('./test-env.cjs').esbuild;
const {root,outputDirectory}=require('./test-env.cjs');
const code=buildSync({stdin:{contents:fs.readFileSync(root+'/src/preferences.ts','utf8').replace("import {invoke} from '@tauri-apps/api/core';",'const invoke=globalThis.invoke;'),loader:'ts'},bundle:true,write:false,format:'cjs',platform:'node'}).outputFiles[0].text;
(async()=>{
 const local=new Map([['unreal-manager.language.v1','ru'],['um-library-page-size','48'],['unrelated','preserve']]);
 const disk={'unreal-manager.language.v1':'ja','unreal-manager.theme.v1':'light'},calls=[];
 const context={exports:{},module:{exports:{}},Promise,localStorage:{getItem:k=>local.get(k)??null,setItem:(k,v)=>local.set(k,v)},window:{__TAURI_INTERNALS__:{},dispatchEvent:()=>{}},CustomEvent:class{},invoke:async(name,args)=>{calls.push({name,args});if(name==='settings_read')return {...disk};if(name==='settings_write')disk[args.key]=args.value;}};
 vm.runInNewContext(code,context);const p=context.module.exports;await p.initializePreferences();
 assert.equal(local.get('unreal-manager.language.v1'),'ja');assert.equal(local.get('unreal-manager.theme.v1'),'light');assert.equal(disk['um-library-page-size'],'48');assert(!('unrelated' in disk));
 p.setPreference('um-plugins-page-size','24');p.setPreference('unreal-manager.language.v1','ko');p.setPreference('um-library-sort','engine-desc');p.setPreference('unreal-manager.theme.v1','dark');
 await new Promise(r=>setTimeout(r,10));assert.equal(disk['unreal-manager.language.v1'],'ko');assert.equal(disk['um-plugins-page-size'],'24');assert.equal(disk['um-library-page-size'],'48');
 local.clear();await p.initializePreferences();assert.equal(p.getPreference('unreal-manager.language.v1'),'ko');assert.equal(p.getPreference('um-plugins-page-size'),'24');assert.equal(p.getPreference('um-library-sort'),'engine-desc');assert.equal(p.getPreference('unreal-manager.theme.v1'),'dark');console.log('PASS native preferences restore including saved library sort and theme, legacy migration, allowlist and independent preference writes');
})().catch(e=>{console.error(e);process.exitCode=1});
