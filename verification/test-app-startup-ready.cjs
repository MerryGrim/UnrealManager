// Actual main.tsx + App mounting, with simulated native IPC only. No app,
// updater helper, user profile, account, installer or external request runs.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const evidence = path.join(__dirname, 'current', `app-startup-ready-${stamp}-${crypto.randomUUID().slice(0, 8)}`);
// Keep Edge's transient profile short; long nested Windows paths break it.
const transientParent = path.resolve(__dirname, 'current');
const temporary = path.join(transientParent, `srt-${crypto.randomUUID().slice(0, 8)}`);
assert.equal(path.dirname(path.resolve(temporary)), transientParent);
assert.match(path.basename(temporary), /^srt-[0-9a-f]{8}$/);
fs.mkdirSync(evidence, {recursive: true}); fs.mkdirSync(temporary);
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY = evidence;
process.env.TEMP = temporary; process.env.TMP = temporary;
const {root, esbuild, playwright, browserLaunchOptions} = require('./test-env.cjs');
const version = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const core = `
export class Channel {constructor(callback){this.onmessage=callback;}}
export async function invoke(command,args={}) {
 window.calls.push({command,args});
 if(command==='settings_read')return new Promise((resolve,reject)=>{window.readSettings={resolve,reject};});
 if(command==='app_update_ui_ready'){
  window.acks.push({mounted:!!document.querySelector('.app-shell'),brand:!!document.querySelector('.workspace-brand'),language:document.documentElement.lang});
  if(window.fixture.rejectAck)throw Error('simulated ACK transport failure');return true;
 }
 if(command==='settings_write'||command==='set_ui_language')return;
 if(command==='app_update_settings_get')return {enabled:true,channel:'stable',configured:true,currentVersion:${JSON.stringify(version)},mode:'installed',supported:true};
 if(['app_update_check','scan_engines','scan_projects'].includes(command))return new Promise(()=>{});
 if(command==='epic_status')return {signedIn:false,displayName:null};
 if(['engine_catalog','engine_pending','engine_resume_tasks','vault_list','vault_resume_tasks'].includes(command))return [];
 if(command==='vault_queue_get')return {revision:0,paused:true,activeId:null,items:[],error:null,completed:null};
 if(command==='vault_epic_status')return {directory:'X:/Fixture/Epic/VaultCache',items:[],warnings:[]};
 if(command==='vault_storage_get')return {vaultPath:'X:/Fixture/Vault',keepEngineBackup:false};
 if(command==='fab_cached_library')return {items:[],cachedAt:1,stale:false};
 if(command==='fab_library')return {items:[],scope:'synthetic-startup'};
 if(command==='vault_discard')return;
 window.unexpected.push(command);throw Error('Unexpected startup IPC: '+command);
}`;
const report = {mockedNative: true, realUpdateVerified: false, checks: [], externalRequests: [], cases: []};
const check = name => report.checks.push({name, passed: true});
async function main() {
 const built = await esbuild.build({
  entryPoints: [path.join(root, 'src/main.tsx')], bundle: true, write: false,
  define: {'process.env.NODE_ENV': '"development"'}, loader: {'.svg':'dataurl','.css':'empty'},
  plugins: [{name:'startup-native-fixture',setup(build){
   build.onResolve({filter:/^@tauri-apps\/api\/(core|event)$/},args=>({path:args.path,namespace:'native-fixture'}));
   build.onLoad({filter:/.*/,namespace:'native-fixture'},args=>({contents:args.path.endsWith('/event')?'export async function listen(){return ()=>{};}':core,loader:'js'}));
   // Deterministic child render failure exercises React's aborted commit,
   // rather than throwing before the actual App starts rendering.
   build.onLoad({filter:/[\\/]EngineResumeTasks\.tsx$/},args=>({contents:fs.readFileSync(args.path,'utf8').replace(' const[tasks,setTasks]', ' if((window as any).fixture.failRender)throw Error("simulated initial child render failure");\n const[tasks,setTasks]'),loader:'tsx'}));
  }}],
 });
 const browser = await playwright.chromium.launch(browserLaunchOptions);
 try {
  for(const kind of ['mounted','preferences-failure','render-failure','ack-failure']) {
   const context = await browser.newContext({viewport:{width:1280,height:800},locale:'en-US',reducedMotion:'reduce'});
   try {
    await context.route('**/*',route=>{
     if(route.request().url()==='http://um-startup.invalid/')return route.fulfill({contentType:'text/html',body:'<!doctype html><html><head><meta charset="utf-8"></head><body><div id="root"></div></body></html>'});
     report.externalRequests.push(route.request().url());return route.abort();
    });
    const page = await context.newPage(); const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto('http://um-startup.invalid/');
    await page.evaluate(kind=>{window.__TAURI_INTERNALS__={};window.fixture={failRender:kind==='render-failure',rejectAck:kind==='ack-failure'};window.calls=[];window.acks=[];window.unexpected=[];},kind);
    await page.addScriptTag({content:built.outputFiles[0].text});
    await page.waitForFunction(()=>!!window.readSettings);
    assert.equal(await page.locator('.app-shell').count(),0);assert.deepEqual(await page.evaluate(()=>window.acks),[]);
    if(kind==='preferences-failure'){
     await page.evaluate(()=>window.readSettings.reject(Error('simulated unreadable local settings')));
     await page.getByText('Could not load settings:',{exact:false}).waitFor();
     assert.equal(await page.locator('.app-shell').count(),0);assert.deepEqual(await page.evaluate(()=>window.acks),[]);
     check('Failed local preference initialization does not mount App or acknowledge health');
    }else{
     await page.evaluate(()=>window.readSettings.resolve({'unreal-manager.language.v1':'en'}));
     if(kind==='render-failure'){
      await page.waitForFunction(()=>!document.querySelector('.app-shell')&&window.calls.some(x=>x.command==='set_ui_language'));
      await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
      assert.deepEqual(await page.evaluate(()=>window.acks),[]);assert.ok(errors.some(error=>error.includes('simulated initial child render failure')));
      check('Failed initial App-child render never acknowledges an aborted React commit');
     }else{
      await page.waitForFunction(()=>window.acks.length===1);
      await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
      assert.deepEqual(await page.evaluate(()=>window.acks),[{mounted:true,brand:true,language:'en'}]);
      const calls=await page.evaluate(()=>window.calls.map(x=>x.command));
      assert.equal(calls.filter(x=>x==='app_update_ui_ready').length,1);
      assert.equal(calls.filter(x=>x==='epic_status').length,2,'Development StrictMode did run App effects twice');
      assert.equal(calls.filter(x=>x==='app_update_check').length,1);
      assert.ok(calls.includes('scan_engines')&&calls.includes('scan_projects'));
      check(kind==='mounted'?'Mounted App acknowledges once under StrictMode while optional network/scans stay pending':'Rejected ACK is not retried automatically or reported as another healthy signal');
     }
    }
    assert.deepEqual(await page.evaluate(()=>window.unexpected),[]);
    if(kind!=='render-failure')assert.deepEqual(errors,[]);
    report.cases.push({kind,calls:await page.evaluate(()=>window.calls.map(x=>x.command)),acks:await page.evaluate(()=>window.acks),expectedRenderFailure:kind==='render-failure'});
   }finally{await context.close();}
  }
  assert.deepEqual(report.externalRequests,[]);report.passed=true;
 }finally{await browser.close();}
}
main().catch(error=>{report.passed=false;report.failure=error.stack;process.exitCode=1;}).finally(()=>{
 // This newly-created transient directory is owned by this process and all
 // browser contexts/children have been awaited closed before removal.
 fs.rmSync(temporary,{recursive:true,force:true});report.temporaryRemoved=!fs.existsSync(temporary);
 fs.writeFileSync(path.join(evidence,'ui-report.json'),JSON.stringify(report,null,2));
 console.log(`${report.passed?'PASS':'FAIL'} ${report.checks.length} startup readiness checks; evidence ${evidence}`);
 if(report.failure)console.error(report.failure);
});
