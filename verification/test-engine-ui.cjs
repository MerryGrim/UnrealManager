const {root,outputDirectory}=require('./test-env.cjs');
const fs=require('fs'),vm=require('vm'),assert=require('assert');const {transformSync}=require('./test-env.cjs').esbuild;

const code=transformSync(fs.readFileSync(root+'/src/EnginesView.tsx','utf8'),{loader:'tsx',format:'cjs',jsx:'automatic'}).code;

let values=[],cursor=0,calls=[],resolveInstall,channel,footer='';const jsx=(type,props)=>({type,props});const react={useState(init){const i=cursor++;if(!(i in values))values[i]=typeof init==='function'?init():init;return [values[i],v=>values[i]=typeof v==='function'?v(values[i]):v];},useRef(init){const i=cursor++;return values[i]??=({current:init});},useEffect(){}};

const releases=[{appName:'UE_5.8',version:'5.8',build:'5.8.3-58210709',installed:[{path:'X:\\Fixture\\Engines\\UE_5.8',version:'5.8.1',updateAvailable:true}]},{appName:'UE_5.7',version:'5.7',build:'5.7.4-1',installed:[]}];

const preview={id:'plan',appName:'UE_5.8',build:'5.8.3',path:'X:\\Fixture\\Engines\\UE_5.8',update:true,components:[{tag:'templates',label:'Шаблоны',bytes:100,selected:true}],bytes:10,downloadBytes:10,changed:1,added:0,removed:0,requiredBytes:10,freeBytes:100,conflicts:[]};const m={exports:{}};

vm.runInNewContext(code,{performance:require('node:perf_hooks').performance,document:{body:{},querySelector:()=>null},module:m,exports:m.exports,require:n=>n==='./i18n'?{tr:x=>x,getLocale:()=>"ru",useLanguage:()=>{}}:n==='react-dom'?{createPortal:child=>child}:n==='react'?react:n==='react/jsx-runtime'?{jsx,jsxs:jsx}:n==='./EngineTransferCard'?{EngineTransferCard:'engine-transfer-card',transferSize:()=>''}:n==='./EngineTile'?{EngineTile:'engine-tile'}:n==='./PageActions'?{PageActions:'actions'}:n==='@tauri-apps/api/core'?{Channel:class{constructor(){channel=this;}},invoke:async(name,args)=>{calls.push({name,args});if(name==='engine_pending')return [];if(name==='engine_catalog')return releases;if(name==='engine_prepare')return {...preview,path:args.path,update:args.update};if(name==='engine_install')return new Promise(r=>resolveInstall=r);}}:{}});

const render=()=>{cursor=0;return m.exports.EnginesView({engines:[{path:'X:\\Fixture\\Engines\\UE_5.8',version:'5.8'}],onRefresh:async()=>{},onProgress:t=>footer=t});};const all=n=>!n?[]:Array.isArray(n)?n.flatMap(all):typeof n==='object'?[n,...all(n.props?.children)]:[];const text=n=>Array.isArray(n)?n.map(text).join(''):typeof n==='string'||typeof n==='number'?String(n):n?.props?text(n.props.children):'';const button=(u,t)=>{const b=all(u).find(n=>n.type==='button'&&text(n)===t);assert(b,t);return b.props;};const tick=()=>new Promise(r=>setImmediate(r));

(async()=>{

 button(render(),'Проверить обновления').onClick();await tick();let u=render();assert(text(u).includes('5.8.1'));assert(all(u).some(n=>n.props?.className==='engine-update-badge'&&text(n).endsWith('5.8.3')));

 button(u,'Обновить').onClick();u=render();assert.equal(all(u).filter(n=>n.type==='input').length,0,'Update never asks for a directory');await tick();u=render();

 const prepared=calls.find(c=>c.name==='engine_prepare');assert.equal(prepared.args.path,'X:\\Fixture\\Engines\\UE_5.8');assert.equal(prepared.args.update,true);assert(!calls.some(c=>c.name==='engine_install'));assert(!text(u).includes('1. Расположение'));

 const modal=all(u).find(n=>n.props?.role==='dialog');assert(text(modal).includes('Размер загрузки'));assert(!all(modal).some(n=>n.type==='input'));button(modal,'Обновить').onClick();button(modal,'Обновить').onClick();await tick();assert.equal(calls.filter(c=>c.name==='engine_install').length,1);

 channel.onmessage({phase:'Скачиваем движок',percent:37,total:100,done:37});u=render();assert.equal(footer.percent,37);all(u).find(n=>n.props?.['aria-label']==='Закрыть').props.onClick();u=render();assert(!all(u).some(n=>n.props?.role==='dialog'));assert.equal(footer.percent,37,'Closing the dialog keeps background progress');resolveInstall('Готово');await tick();await tick();u=render();assert.equal(footer,null);

 button(u,'Доступные').onClick();u=render();all(u).filter(n=>n.type==='engine-tile')[1].props.onInstall();u=render();
 assert.equal(all(u).find(n=>n.type==='input').props.value,'','A new install waits for an explicit destination');
 assert.equal(button(u,'Далее').disabled,true);const prepareCount=calls.filter(c=>c.name==='engine_prepare').length;
 button(u,'Далее').onClick();await tick();assert.equal(calls.filter(c=>c.name==='engine_prepare').length,prepareCount,'Empty destinations cannot prepare an install');
 all(u).find(n=>n.type==='input').props.onChange({target:{value:'   '}});u=render();assert.equal(button(u,'Далее').disabled,true);
 all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.7'}});u=render();assert.equal(button(u,'Далее').disabled,false);
 button(u,'Далее').onClick();await tick();u=render();const newInstall=calls.filter(c=>c.name==='engine_prepare').at(-1);assert.equal(newInstall.args.update,false);assert.equal(newInstall.args.path,'X:/Fixture/Engines/UE_5.7');assert.equal(all(u).filter(n=>n.type==='input'&&n.props.type==='checkbox').length,2,'New installs retain base and optional component choices');console.log('PASS engine UI: supplied update path, explicit new destination, empty path guard, components, duplicate guard, background progress and close');

})().catch(e=>{console.error(e);process.exitCode=1;});

