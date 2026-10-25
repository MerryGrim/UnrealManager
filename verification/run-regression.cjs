// Canonical mocked-UI regression selection. No native application is started.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const {spawnSync}=require('node:child_process');
const selection=Object.freeze([
 'test-locale-catalogues.cjs','test-preferences.cjs','test-library-topics.cjs',
 'test-vault-all-kinds.cjs','test-beta5.cjs','test-resume-forget.cjs',
 'test-engine-action-layout.cjs','test-beta4.cjs','test-engine-ui.cjs',
 'test-engine-recovery-progress.cjs','test-app-updates.cjs','test-app-startup-ready.cjs','test-startup-window-failure.cjs',
 'test-accessibility-matrix.cjs','test-workspace-header.cjs','test-epic-auth-required.cjs','test-project-discovery.cjs','test-redemption-batch.cjs','test-redemption-bridge.cjs',
]);
const source=path.resolve(__dirname,'../source'),current=path.resolve(__dirname,'current');
const sha=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function main(argv=process.argv.slice(2)){
 if(argv.length===1&&argv[0]==='--list'){console.log(selection.join('\n'));return;}
 const validateOnly=argv.includes('--validate'),requested=argv.filter(v=>v!=='--validate');
 const tests=requested.length?requested:[...selection];
 if(new Set(tests).size!==tests.length||tests.some(test=>!selection.includes(test)))throw Error(`Select each known test once: ${selection.join(', ')}`);
 for(const test of tests)if(!fs.statSync(path.join(__dirname,test)).isFile())throw Error(`Missing test: ${test}`);
 // Resolve existing tools only: validation installs nothing and opens no browser.
 const environment=require('./test-env.cjs'),esbuild=environment.esbuild.version;
 if(!environment.playwright.chromium?.launch)throw Error('Playwright has no Chromium launcher');
 const startedAt=new Date().toISOString(),parent=path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY||current);
 if(parent!==current&&!parent.startsWith(current+path.sep))throw Error('Regression evidence must stay under verification/current');
 fs.mkdirSync(parent,{recursive:true});
 const outputDirectory=path.join(parent,`regression-${startedAt.replace(/[:.]/g,'-')}-${crypto.randomUUID().slice(0,8)}`);fs.mkdirSync(outputDirectory);
 const results=[],planned=tests.map(test=>({test,sha256:sha(path.join(__dirname,test))}));
 if(!validateOnly)for(const test of tests){
  const testStarted=new Date().toISOString(),directory=path.join(outputDirectory,path.basename(test,'.cjs'));fs.mkdirSync(directory);console.log(`RUN ${test}`);
  const result=spawnSync(process.execPath,[path.join(__dirname,test)],{cwd:source,env:{...process.env,UM_VERIFICATION_OUTPUT_DIRECTORY:directory},encoding:'utf8',timeout:180000,maxBuffer:16*1024*1024,windowsHide:true});
  const output=(result.stdout||'')+(result.stderr||'')+(result.error?`\n${result.error.stack}\n`:'');
  const log=path.join(directory,'output.log');fs.writeFileSync(log,output,{flag:'wx'});if(output)process.stdout.write(output);
  const reportedEvidenceDirectories=[...output.matchAll(/evidence\s+([^\r\n]+)/gi)].map(m=>path.resolve(m[1].trim())).filter(p=>p.startsWith(current+path.sep));
  const passed=result.status===0&&!result.error;results.push({test,startedAt:testStarted,finishedAt:new Date().toISOString(),passed,exitCode:result.status,signal:result.signal,log,directory,reportedEvidenceDirectories});console.log(`${passed?'PASS':'FAIL'} ${test}`);
 }
 const report={schema:1,version:JSON.parse(fs.readFileSync(path.join(source,'package.json'),'utf8')).version,mode:validateOnly?'validation-only':'mocked-ui',startedAt,finishedAt:new Date().toISOString(),node:process.version,esbuild,outputDirectory,planned,results,passed:validateOnly?null:results.every(r=>r.passed),validated:validateOnly,realEpicOrNativeUpdateVerified:false};
 const reportPath=path.join(outputDirectory,'regression-report.json');fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(`Report: ${reportPath}`);process.exitCode=validateOnly||report.passed?0:1;return report;
}
module.exports={selection,main};
if(require.main===module){try{main();}catch(error){console.error(error.stack);process.exitCode=1;}}
