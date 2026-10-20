const {root,outputDirectory}=require('./test-env.cjs');
const fs=require('fs'),vm=require('vm'),assert=require('assert');const {transformSync}=require('./test-env.cjs').esbuild;

const code=transformSync(fs.readFileSync(root+'/src/EnginesView.tsx','utf8'),{loader:'tsx',format:'cjs',jsx:'automatic'}).code;

let values=[],cursor=0,calls=[],resolveInstall,resolvePrepare,holdPrepare=false,prepareError='',channel,footer='';const jsx=(type,props)=>({type,props});const react={useState(init){const i=cursor++;if(!(i in values))values[i]=typeof init==='function'?init():init;return [values[i],v=>values[i]=typeof v==='function'?v(values[i]):v];},useRef(init){const i=cursor++;return values[i]??=({current:init});},useEffect(){}};

const releases=[{appName:'UE_5.8',version:'5.8',build:'5.8.3-58210709',installed:[{path:'X:\\Fixture\\Engines\\UE_5.8',version:'5.8.1',updateAvailable:true}]},{appName:'UE_5.7',version:'5.7',build:'5.7.4-1',installed:[]}];

const preview={id:'plan',appName:'UE_5.8',build:'5.8.3',path:'X:\\Fixture\\Engines\\UE_5.8',update:true,components:[{tag:'templates',label:'Шаблоны',bytes:100,selected:true}],bytes:10,downloadBytes:10,changed:1,added:0,removed:0,requiredBytes:10,freeBytes:100,conflicts:[]};const m={exports:{}};

const localized={exports:{}};
vm.runInNewContext(transformSync(fs.readFileSync(root+'/src/i18n.ts','utf8'),{loader:'ts',format:'cjs'}).code,{module:localized,exports:localized.exports,navigator:{language:'ru-RU'},require:n=>n==='./preferences'?{getPreference:()=> 'ru',setPreference:()=>{}}:n==='react'?{}:n==='@tauri-apps/api/core'?{invoke:async()=>{}}:JSON.parse(fs.readFileSync(root+'/src/'+n.replace(/^\.\//,''),'utf8'))});
const i18n={tr:localized.exports.tr,getLocale:localized.exports.getLocale,useLanguage:()=>{}};
const transfer={exports:{}};
vm.runInNewContext(transformSync(fs.readFileSync(root+'/src/EngineTransferCard.tsx','utf8'),{loader:'tsx',format:'cjs',jsx:'automatic'}).code,{module:transfer,exports:transfer.exports,require:n=>n==='./i18n'?i18n:n==='react/jsx-runtime'?{jsx,jsxs:jsx}:{}});

vm.runInNewContext(code,{performance:require('node:perf_hooks').performance,document:{body:{},querySelector:()=>null},module:m,exports:m.exports,require:n=>n==='./i18n'?i18n:n==='react-dom'?{createPortal:child=>child}:n==='react'?react:n==='react/jsx-runtime'?{jsx,jsxs:jsx}:n==='./EngineTransferCard'?{...transfer.exports,EngineTransferCard:'engine-transfer-card'}:n==='./EngineTile'?{EngineTile:'engine-tile'}:n==='./PageActions'?{PageActions:'actions'}:n==='@tauri-apps/api/core'?{Channel:class{constructor(){channel=this;}},invoke:async(name,args)=>{calls.push({name,args});if(name==='engine_pending')return [];if(name==='engine_catalog')return releases;if(name==='engine_prepare'){if(prepareError)throw prepareError;const prepared={...preview,appName:args.appName,path:args.path,update:args.update,components:preview.components.map(c=>({...c,selected:args.selected===null?c.selected:args.selected.includes(c.tag)}))};if(holdPrepare)return new Promise(resolve=>resolvePrepare=()=>{holdPrepare=false;resolve(prepared);});return prepared;}if(name==='engine_resume_prepare')return {...preview,id:args.id,appName:'UE_5.7',path:'X:/Fixture/Engines/UE_5.7-saved',update:false};if(name==='engine_install')return new Promise(r=>resolveInstall=r);}}:{}});

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
 button(u,'Далее').onClick();await tick();u=render();const newInstall=calls.filter(c=>c.name==='engine_prepare').at(-1);assert.equal(newInstall.args.update,false);assert.equal(newInstall.args.path,'X:/Fixture/Engines/UE_5.7');assert.equal(all(u).filter(n=>n.type==='input'&&n.props.type==='checkbox').length,2,'Step 2 retains base and optional component choices');
 assert(all(u).some(n=>n.type==='span'&&n.props.className==='active'&&text(n)==='2. Компоненты'));
 const discardsBeforeBack=calls.filter(c=>c.name==='engine_discard').length,installsBeforeBack=calls.filter(c=>c.name==='engine_install').length;
 button(u,'1. Расположение').onClick();u=render();assert.equal(all(u).find(n=>n.type==='input').props.value,'X:/Fixture/Engines/UE_5.7','Completed location step returns to the chosen destination');
 all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.7-other'}});u=render();button(u,'Далее').onClick();await tick();u=render();
 assert.equal(calls.filter(c=>c.name==='engine_prepare').at(-1).args.path,'X:/Fixture/Engines/UE_5.7-other');assert.deepEqual(Array.from(calls.filter(c=>c.name==='engine_prepare').at(-1).args.selected),['templates'],'Changing destination recomputes the plan with the selected component tags');
 const priorStep=button(u,'1. Расположение');holdPrepare=true;all(u).filter(n=>n.type==='input'&&n.props.type==='checkbox')[1].props.onChange({target:{checked:false}});u=render();
 assert.equal(button(u,'1. Расположение').disabled,true,'Earlier step is disabled while recomputing');assert(!all(u).some(n=>n.type==='button'&&text(n)==='Назад'),'Busy recomputation has no Back action');
 const busyPrepareCount=calls.filter(c=>c.name==='engine_prepare').length;priorStep.onClick();button(u,'1. Расположение').onClick();u=render();assert(all(u).some(n=>n.props?.className==='engine-components'),'Stale and disabled prior-step handlers cannot navigate while busy');assert.equal(calls.filter(c=>c.name==='engine_prepare').length,busyPrepareCount);
 resolvePrepare();await tick();u=render();button(u,'Назад').onClick();u=render();all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.7-empty-selection'}});u=render();button(u,'Далее').onClick();await tick();u=render();
 assert.deepEqual(Array.from(calls.filter(c=>c.name==='engine_prepare').at(-1).args.selected),[],'An explicitly empty component selection survives Back and destination changes');assert.equal(all(u).filter(n=>n.type==='input'&&n.props.type==='checkbox')[1].props.checked,false);
 assert.equal(calls.filter(c=>c.name==='engine_discard').length,discardsBeforeBack,'Back does not discard a plan or saved task');assert.equal(calls.filter(c=>c.name==='engine_install').length,installsBeforeBack,'Navigation never starts installation');
 all(u).find(n=>n.props?.['aria-label']==='Закрыть').props.onClick();u=render();all(u).filter(n=>n.type==='engine-tile')[0].props.onInstall();u=render();all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.8-new'}});u=render();button(u,'Далее').onClick();await tick();u=render();
 assert.equal(calls.filter(c=>c.name==='engine_prepare').at(-1).args.selected,null,'A fresh release selection resets the remembered component tags');
 all(u).find(n=>n.props?.['aria-label']==='Закрыть').props.onClick();u=render();const prepareCountBeforeResume=calls.filter(c=>c.name==='engine_prepare').length;
 all(u).find(n=>n.props?.onResume).props.onResume('saved-task');await tick();u=render();const resumed=all(u).find(n=>n.props?.role==='dialog');assert(text(resumed).includes('X:/Fixture/Engines/UE_5.7-saved'));assert(!all(resumed).some(n=>n.type==='button'&&['Назад','1. Расположение'].includes(text(n))),'A saved task has no Back or editable prior step');assert(all(resumed).filter(n=>n.type==='input').every(n=>n.props.type==='checkbox'&&n.props.disabled),'Saved task parameters remain read-only');button(resumed,'Продолжить установку');assert.equal(calls.filter(c=>c.name==='engine_prepare').length,prepareCountBeforeResume,'Resuming does not reprepare a different path');assert(!calls.some(c=>c.name==='engine_resume_forget'),'Navigation never forgets a saved task');
 all(resumed).find(n=>n.props?.['aria-label']==='Закрыть').props.onClick();u=render();all(u).filter(n=>n.type==='engine-tile')[1].props.onInstall();u=render();all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.7'}});u=render();button(u,'Далее').onClick();await tick();u=render();
 button(u,'Скачать и установить').onClick();u=render();
 let installModal=all(u).find(n=>n.props?.role==='dialog');
 assert.equal(all(installModal).filter(n=>n.type==='input'&&n.props.type==='checkbox').length,0,'Step 3 hides component choices immediately, before native progress arrives');
 assert(!all(installModal).some(n=>n.props?.className==='engine-components'||n.props?.className==='update-summary'),'Step 3 hides component selection and its size summary');
 assert(all(installModal).some(n=>n.props?.className==='project-delete-path'&&text(n)==='X:/Fixture/Engines/UE_5.7'),'Step 3 retains the chosen destination');
 assert(all(installModal).some(n=>n.props?.role==='status'&&text(n).includes('Сохраняем задачу установки')),'Step 3 shows its initial status');
 assert(all(installModal).some(n=>n.type==='progress'),'Step 3 retains progress display');
 assert(all(installModal).some(n=>n.type==='span'&&n.props.className==='active'&&text(n)==='3. Установка'));
 for(const [phase,label,bytes] of [['Проверяем сохранённую задачу','Записи',false],['Проверяем наличие блоков','Блоки',false],['Проверяем сохранённые блоки','Блоки',true],['Проверяем установленные файлы','Файлы',true]]){
  const progress={phase,percent:25,total:100,done:25,filesDone:25,filesTotal:100,networkBytes:0,bytesPerSecond:0,processedBytes:bytes?1048576:0,totalBytes:bytes?4194304:0,processingBytesPerSecond:bytes?524288:0};
  channel.onmessage(progress);u=render();installModal=all(u).find(n=>n.props?.role==='dialog');
  const status=all(installModal).find(n=>n.props?.role==='status');assert(text(status).includes(`${label}: 25 / 100`),phase+' uses the actual counter unit');assert(text(status).includes('25%'));
  assert.equal(all(status).find(n=>n.type==='progress').props.value,25);
  if(bytes){assert(text(status).includes('Обработано: 1 МБ / 4 МБ'));assert(text(status).includes('Скорость обработки: ')&&text(status).includes('МБ/с'));}else assert(!text(status).includes('Обработано'));
  const card=transfer.exports.EngineTransferCard({progress:{...progress,title:'Unreal Engine 5.7.4'},onOpen:()=>{}});
  assert(all(card).some(n=>n.type==='div'&&n.props.children?.[0]?.type==='small'&&text(n.props.children[0])===label&&text(n.props.children[1])==='25 / 100'),'Transfer card counter unit '+phase);
  assert.equal(all(card).find(n=>n.type==='progress').props.value,25);
  assert.equal(text(card).includes('Скорость обработки'),bytes,'Transfer card only displays measured processing bytes');
 }
 channel.onmessage({phase:'Считаем временные файлы',done:0,total:0,percent:0,filesDone:7,filesTotal:0,processedBytes:0,totalBytes:0,networkBytes:0});u=render();installModal=all(u).find(n=>n.props?.role==='dialog');
 const counting=all(installModal).find(n=>n.props?.role==='status');assert(text(counting).includes('Найдено объектов: 7'));assert(!text(counting).includes('%'));assert.equal(all(counting).find(n=>n.type==='progress').props.value,undefined);
 const countingCard=transfer.exports.EngineTransferCard({progress:{phase:'Считаем временные файлы',done:0,total:0,percent:0,filesDone:7,filesTotal:0,networkBytes:0,bytesPerSecond:0,title:'Unreal Engine'},onOpen:()=>{}});assert(text(countingCard).includes('Найдено объектов7'));assert.equal(all(countingCard).find(n=>n.type==='progress').props.value,undefined);
 const verifying={phase:'Проверяем установленные файлы',done:1086,total:250332,percent:0,filesDone:1086,filesTotal:250332,processedBytes:3145728,totalBytes:0,filesPerSecond:47.5,readBytesPerSecond:524288,currentFile:'Engine/Content/VeryLongFolder/CurrentAsset.uasset',networkBytes:0,bytesPerSecond:0};
 channel.onmessage(verifying);u=render();installModal=all(u).find(n=>n.props?.role==='dialog');
 const verifiedStatus=all(installModal).find(n=>n.props?.role==='status'),verifiedCard=transfer.exports.EngineTransferCard({progress:{...verifying,title:'Unreal Engine'},onOpen:()=>{}});
 for(const tree of [verifiedStatus,verifiedCard]){
  assert(text(tree).includes('0,4%'),'Fractional count progress is localized and does not reuse truncated native percent');
  assert(text(tree).includes('Прочитано')&&text(tree).includes('3 МБ'),'Actual read bytes are visible without an unknown byte denominator');
  assert(text(tree).includes('Скорость чтения')&&text(tree).includes('Скорость проверки'));
  assert(all(tree).some(n=>n.props?.title===verifying.currentFile),'Current relative file is visible with a full accessible title');
 }
 assert.equal(all(verifiedStatus).find(n=>n.type==='progress').props.value,1086/250332*100);
 assert.equal(transfer.exports.enginePercentText({...verifying,done:250331}),'99,9%','Do not round unfinished work to 100%');
 assert.equal(transfer.exports.enginePercentText({...verifying,done:250332}),'100%');
 assert.equal(transfer.exports.engineProgressPercent({...verifying,total:0}),undefined);
 assert.equal(transfer.exports.enginePercentText({...verifying,done:1}),'<0,1%');
 assert.equal(transfer.exports.engineCountLabel('Fab: Проверяем сохранённые блоки'),'Блоки');
 const applying={...verifying,phase:'Устанавливаем движок',done:532,total:12475,filesDone:532,filesTotal:12475,processedBytes:134217728,totalBytes:1073741824,readBytes:7340032,readBytesPerSecond:1048576,currentFileDone:3145728,currentFileTotal:12582912};
 for(const[stage,label]of [['readOld','Проверяем прежний файл'],['readNew','Проверяем новый файл'],['readBoth','Проверяем прежний и новый файлы'],['resumeCheck','Проверяем файл после возобновления']]){
  const snapshot={...applying,currentFileStage:stage};channel.onmessage(snapshot);u=render();const modalStatus=all(all(u).find(n=>n.props?.role==='dialog')).find(n=>n.props?.role==='status');
  const card=transfer.exports.EngineTransferCard({progress:{...snapshot,title:'Unreal Engine'},onOpen:()=>{}});
  for(const tree of [modalStatus,card]){const meters=all(tree).filter(n=>n.type==='progress');assert.equal(meters.length,2);assert.equal(meters[0].props.value,532/12475*100);assert.equal(meters[1].props.value,25);assert.equal(meters[1].props['aria-label'],'Прогресс текущего файла');assert(text(tree).includes(label)&&text(tree).includes('3 МБ / 12 МБ · 25%'));}
  assert.equal(transfer.exports.engineProgressMetrics(snapshot).find(r=>r.label==='Прочитано').value,'7 МБ','Disk reads come from readBytes, not logical completed output');
 }
 channel.onmessage({...applying,currentFileStage:'readBoth',currentFileDone:9437184,readBytes:13631488});u=render();const partialStatus=all(all(u).find(n=>n.props?.role==='dialog')).find(n=>n.props?.role==='status');assert.deepEqual(all(partialStatus).filter(n=>n.type==='progress').map(n=>n.props.value),[532/12475*100,75],'Inside-file progress advances while completed file count stays fixed');
 channel.onmessage({...applying,currentFileStage:'replace',currentFileDone:12582912,currentFileTotal:undefined});u=render();const replaceStatus=all(all(u).find(n=>n.props?.role==='dialog')).find(n=>n.props?.role==='status');assert(text(replaceStatus).includes('Заменяем файл'));assert.equal(all(replaceStatus).filter(n=>n.type==='progress').length,1,'Rename has no invented byte meter or completion percentage');
 assert.equal(transfer.exports.engineProgressMetrics({...applying,paused:true}).find(r=>r.label==='Скорость чтения').value,'0 МБ/с');
 assert(!transfer.exports.engineProgressMetrics({...applying,readBytes:undefined}).some(r=>r.label==='Прочитано'),'Logical APPLY prefix never becomes disk read bytes');
 assert(!transfer.exports.engineProgressMetrics(applying).some(r=>r.label==='Скорость обработки'),'Logical completion-prefix speed is omitted when actual read metrics are available');
 assert.equal(transfer.exports.engineProgressMetrics({...applying,currentFileStage:'readOld',currentFileTotal:undefined}).find(r=>r.fileProgress).fileProgress.value,undefined,'Unknown in-file byte total stays indeterminate');
 assert(!transfer.exports.engineProgressMetrics({...applying,currentFile:undefined,currentFileStage:'readBoth'}).some(r=>r.fileProgress),'Phase exit clears the per-file meter when native clears currentFile');
 channel.onmessage({phase:'Скачиваем движок',percent:41,total:100,done:41,networkBytes:1048576});u=render();installModal=all(u).find(n=>n.props?.role==='dialog');
 assert.equal(all(installModal).filter(n=>n.type==='input'&&n.props.type==='checkbox').length,0,'Active downloading keeps the component list hidden');
 assert(text(installModal).includes('X:/Fixture/Engines/UE_5.7'),'Active progress retains the chosen destination');
 assert(all(installModal).some(n=>n.props?.role==='status'&&text(n).includes('Скачиваем движок')&&text(n).includes('41%')));
 assert.equal(all(installModal).find(n=>n.type==='progress').props.value,41);
 resolveInstall('5.7.4-51494982+++UE5+Release-5.7-Windows установлена. Временные файлы очищены.\nОбщий список Epic не обновлён. Установленный движок доступен в Unreal Manager.');await tick();await tick();localized.exports.setLanguagePreference('en');u=render();
 const completed=all(u).find(n=>n.props?.role==='status'&&text(n).includes('installed. Temporary files cleaned.'));
 assert(completed,'Actual English i18n translates the completed installation result');
 assert.equal(text(completed),'5.7.4-51494982+++UE5+Release-5.7-Windows installed. Temporary files cleaned.\nThe shared Epic list could not be updated. The installed engine is available in Unreal Manager.');
 assert.equal(transfer.exports.formatEnginePhase('Fab: Проверяем файлы Vault'),'Fab: Verifying Vault files');
 assert(!/[А-Яа-яЁё]/.test(text(completed)),'Success and warning lines contain no Cyrillic in English');
 all(u).find(n=>n.props?.['aria-label']===i18n.tr('Закрыть')).props.onClick();u=render();all(u).filter(n=>n.type==='engine-tile')[1].props.onInstall();u=render();all(u).find(n=>n.type==='input').props.onChange({target:{value:'X:/Fixture/Engines/UE_5.7-guard'}});u=render();
 prepareError='Не удалось определить, какой движок использует UnrealEditor-Cmd.exe. Закройте процесс или повторите проверку.';button(u,i18n.tr('Далее')).onClick();await tick();u=render();
 const nativeGuardAlert=all(u).find(n=>n.props?.role==='alert');assert(nativeGuardAlert,'Native formatted guard error is displayed');assert.equal(text(nativeGuardAlert),'Could not determine which engine is using UnrealEditor-Cmd.exe. Close the process or check again.');assert(!/[А-Яа-яЁё]/.test(text(nativeGuardAlert)),'The dynamic guard message is fully translated without a Cyrillic leak');
 console.log('PASS APPLY inside-file meters/stage labels, real read-byte separation, paused rate zero and no rename meter; completed files stay fixed until native success');
 console.log('PASS fractional progress and matching card/modal read bytes, file rate/current file, unknown denominator and completion boundaries');
 console.log('PASS actual English i18n renders the formatted native guard error and preserves its process name');
 console.log('PASS engine UI: pre-start Back and clickable prior step, preserved component tags and changed-path reprepare, fresh-release reset, busy navigation guard, read-only resume, supplied update path, explicit destination, step 3 without checkboxes, translated completion, duplicate guard and background progress');

})().catch(e=>{console.error(e);process.exitCode=1;});

