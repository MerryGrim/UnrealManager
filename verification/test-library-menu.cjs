const {root,outputDirectory}=require('./test-env.cjs');
const fs=require('fs'),vm=require('vm'),assert=require('assert');
const {transformSync}=require('./test-env.cjs').esbuild;
const code=transformSync(fs.readFileSync(root+'/src/LibraryTile.tsx','utf8'),{loader:'tsx',format:'cjs',jsx:'automatic'}).code;
let states=[],cursor=0,opened=[],requests=[],refreshes=0;const out={exports:{}};
const jsx=(type,props)=>({type,props});
const react={useRef:()=>({current:null}),useEffect:()=>{},useState:init=>{const i=cursor++;if(!(i in states))states[i]=init;return[states[i],v=>states[i]=typeof v==='function'?v(states[i]):v];}};
vm.runInNewContext(code,{module:out,exports:out.exports,window:{innerWidth:1200,innerHeight:800},document:{body:{}},require:n=>n==='react'?react:n==='react/jsx-runtime'?{jsx,jsxs:jsx,Fragment:'fragment'}:n==='./i18n'?{tr:x=>x,getLocale:()=>"ru",useLanguage:()=>{}}:n==='react-dom'?{createPortal:n=>n}:n==='@tauri-apps/api/core'?{invoke:async(name,args)=>{requests.push({name,args});return name==='vault_delete_preview'?['X:/Fixture/Vault/package']:undefined;}}:n==='./FabDetails'?{FabDetails:'details'}:n==='./EpicLibrary'?{FabCover:'cover'}:{}});
function all(n){if(!n)return[];if(Array.isArray(n))return n.flatMap(all);if(typeof n!=='object')return[];return[n,...all(n.props?.children)];}
const label=n=>Array.isArray(n)?n.map(label).join(''):typeof n==='string'?n:n?.props?label(n.props.children):'';
(async()=>{
 for(const [kind,title] of [['Asset','Добавить в проект'],['Project','Создать проект'],['Plugin','Скачать в хранилище']]){
  states=[];const item={id:kind,title:kind,kind};const props={item,onOpen:(...args)=>opened.push(args),busy:false,progress:null,downloaded:false};
  const render=()=>{cursor=0;return out.exports.LibraryTile(props);};let ui=render();all(ui).find(n=>n.type==='button').props.onContextMenu({preventDefault(){},clientX:1100,clientY:750});ui=render();
  const rows=all(ui).filter(n=>n.props?.role==='menuitem');assert.equal(rows.length,kind==='Plugin'?3:4);assert.equal(label(rows[1]),title);assert.equal(label(rows.at(-1)),'Открыть расположение');
  rows[1].props.onClick();assert.equal(opened.at(-1)[0].id,kind);assert.equal(opened.at(-1)[1],kind!=='Plugin');if(kind==='Plugin')assert.equal(opened.at(-1)[2],true);
  ui=render();all(ui).find(n=>n.type==='button').props.onContextMenu({preventDefault(){},clientX:10,clientY:10});ui=render();await all(ui).find(n=>n.props?.role==='menuitem'&&label(n)==='Открыть расположение').props.onClick();ui=render();assert(all(ui).some(n=>n.props?.role==='alertdialog'));assert.equal(requests.length,0);
 }
 states=[];const props={item:{id:'asset',title:'Asset',kind:'Asset'},onOpen(){},busy:false,progress:null,downloaded:true,packageId:'downloaded-package',packageIds:['downloaded-package','second-version'],onVaultChanged:async()=>{refreshes++;}};
 cursor=0;let ui=out.exports.LibraryTile(props);all(ui).find(n=>n.type==='button').props.onContextMenu({preventDefault(){},clientX:10,clientY:10});cursor=0;ui=out.exports.LibraryTile(props);await all(ui).find(n=>n.props?.role==='menuitem'&&label(n)==='Открыть расположение').props.onClick();assert.equal(requests[0].name,'vault_reveal');assert.equal(requests[0].args.id,'downloaded-package');
 cursor=0;ui=out.exports.LibraryTile(props);all(ui).find(n=>n.type==='button').props.onContextMenu({preventDefault(){},clientX:10,clientY:10});cursor=0;ui=out.exports.LibraryTile(props);
 const menu=all(ui).filter(n=>n.props?.role==='menuitem');assert.equal(menu.length,5);assert.equal(label(menu[4]),'Удалить из хранилища');menu[4].props.onClick();await new Promise(setImmediate);cursor=0;ui=out.exports.LibraryTile(props);
 assert(all(ui).some(n=>n.props?.role==='alertdialog'));assert(!requests.some(r=>r.name==='vault_delete'));all(ui).find(n=>n.type==='button'&&label(n)==='Отмена').props.onClick();cursor=0;ui=out.exports.LibraryTile(props);assert(!all(ui).some(n=>n.props?.role==='alertdialog'));
 all(ui).find(n=>n.type==='button').props.onContextMenu({preventDefault(){},clientX:10,clientY:10});cursor=0;ui=out.exports.LibraryTile(props);all(ui).find(n=>n.props?.role==='menuitem'&&label(n)==='Удалить из хранилища').props.onClick();await new Promise(setImmediate);cursor=0;ui=out.exports.LibraryTile(props);all(ui).find(n=>n.type==='button'&&label(n)==='Удалить навсегда').props.onClick();await new Promise(setImmediate);
 const removal=requests.find(r=>r.name==='vault_delete');assert.deepEqual(Array.from(removal.args.ids),['downloaded-package','second-version']);assert.deepEqual(Array.from(removal.args.expectedPaths),['X:/Fixture/Vault/package']);assert.equal(refreshes,1);
 console.log('PASS: deletion confirmation/cancel, exact package IDs and paths, refresh after removal.');
 console.log('PASS: correct two-row menu for all types; install intent; missing-download error dialog; downloaded package location request.');
})().catch(e=>{console.error(e);process.exitCode=1;});
