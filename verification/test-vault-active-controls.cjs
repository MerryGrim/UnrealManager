const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const env = require('./test-env.cjs');

const config = JSON.parse(fs.readFileSync(path.join(env.root, 'src-tauri/tauri.conf.json'), 'utf8'));
assert.equal(config.app.windows.find(item => (item.label || 'main') === 'main')?.dragDropEnabled, false,
    'WebView2 must pass HTML5 queue drag/drop events to the renderer');

const core = `
const copy = value => JSON.parse(JSON.stringify(value));
function snapshot() { return copy(window.queue); }
function publish() {
    const value = snapshot();
    for (const fn of window.nativeListeners?.['vault-queue-changed'] || []) fn({payload:value});
    return value;
}
window.deliverQueue = value => { window.queue = copy(value); return publish(); };
export class Channel { constructor(fn) { this.onmessage = fn; } }
export async function invoke(name, args) {
    window.calls.push({name,args});
    if (name === 'vault_queue_get') return snapshot();
    if (name === 'vault_resume_tasks') {
        const value=copy(window.savedTasks || []);
        window.resumeReads=(window.resumeReads || 0)+1;
        if (window.resumeReads === window.holdResumeNumber) return new Promise(resolve => {window.deliverOldResume=() => resolve(value);});
        return value;
    }
    if (name === 'vault_list' || name === 'engine_resume_tasks') return [];
    if (name === 'fab_cached_library') return {items:[]};
    if (name === 'vault_epic_status') return {directory:'',items:[],warnings:[]};
    if (name === 'settings_read') return {};
    if (name === 'settings_write' || name === 'vault_discard') return;
    if (['vault_pause','vault_cancel','vault_queue_reorder'].includes(name)) throw new Error('Unscoped or pending-only command used for an active queue item: '+name);
    if (['vault_queue_pause_active','vault_queue_cancel_active','vault_queue_defer_active'].includes(name)) {
        const queue = window.queue;
        if (args.expectedRevision !== queue.revision) throw new Error('Очередь изменилась. Повторите действие.');
        const active = queue.items.find(item => item.id === queue.activeId);
        if (!active || args.id !== active.id) throw new Error('Active task identity changed');
        if (active.controlPending) throw new Error('A checkpoint control is already pending');
        if (name === 'vault_queue_pause_active') {
            if (!active.progress?.canPause || typeof args.paused !== 'boolean') throw new Error('Pause unavailable');
            active.progress = {...active.progress, paused:args.paused, bytesPerSecond:args.paused ? 0 : 1048576};
            queue.revision++;
        } else {
            const defer = name === 'vault_queue_defer_active';
            if (!(defer ? active.canDefer : active.canCancel)) throw new Error('Active control unavailable');
            if (defer && args.beforeId !== null && !queue.items.some(item => item.id === args.beforeId && item.status === 'pending')) throw new Error('Invalid pending destination');
            if (defer && args.beforeId === queue.items.find(item => item.status === 'pending')?.id) throw new Error('Верните загрузку после другого задания очереди.');
            active.controlPending = defer ? 'defer' : 'cancel';
            active.canDefer = active.canCancel = false;
            active.progress = {...active.progress, canPause:false};
            queue.revision++;
        }
        return publish();
    }
    throw new Error('Unexpected native command in active-controls fixture: '+name);
}`;
const event = `export async function listen(name, fn) {
    (window.nativeListeners ||= {})[name] ||= [];
    window.nativeListeners[name].push(fn);
    return () => { window.nativeListeners[name] = window.nativeListeners[name].filter(item => item !== fn); };
}`;
const harness = `
import React from 'react';
import {createRoot} from 'react-dom/client';
import {useVault} from './src/Vault';
import {setLanguagePreference} from './src/i18n';
setLanguagePreference('ru');
function Harness() {
    const vault = useVault([], () => {}, [], () => {}, null);
    return <>{vault.view}{vault.dialog}</>;
}
createRoot(document.getElementById('root')).render(<React.StrictMode><Harness/></React.StrictMode>);`;

const task = (id, status = 'pending') => ({
    id, assetId:'asset-'+id, artifactId:'artifact-'+id, selectedEngine:'5.8', title:'Fixture '+id.toUpperCase(),
    requestedVaultPath:'X:/Fixture/Vault', status, packageId:status === 'downloading' ? id.repeat(40) : null,
    error:null, progress:status === 'downloading' ? {
        phase:'Загружаем и проверяем файлы',done:3*1048576,total:8*1048576,filesDone:3,filesTotal:8,
        networkBytes:5*1048576,bytesPerSecond:1048576,paused:false,canPause:true,
    } : null, canDefer:status === 'downloading',canCancel:status === 'downloading',controlPending:null,
});
const initial = () => ({revision:21,paused:false,activeId:'a',items:[task('a','downloading'),task('b'),task('c')],error:null,completed:null});
const afterCheckpoint = (ids, activeId, revision = 23) => ({
    revision,paused:false,activeId,items:ids.map(id => {
        const item = task(id,id === activeId ? 'downloading' : 'pending');
        if (id === 'a' && id !== activeId) Object.assign(item,{packageId:'a'.repeat(40),progress:{...task('a','downloading').progress,paused:true,canPause:false,bytesPerSecond:0}});
        return item;
    }),error:null,completed:null,
});

(async () => {
    const bundle = await env.esbuild.build({stdin:{contents:harness,resolveDir:env.root,loader:'tsx'},bundle:true,write:false,
        loader:{'.svg':'dataurl'},plugins:[{name:'synthetic-native',setup(builder) {
            builder.onResolve({filter:/^@tauri-apps\/api\/(core|event)$/}, args => ({path:args.path,namespace:'fixture'}));
            builder.onLoad({filter:/.*/,namespace:'fixture'}, args => ({contents:args.path.endsWith('/event') ? event : core,loader:'js'}));
        }}]});
    const browser = await env.playwright.chromium.launch(env.browserLaunchOptions);
    const checks = [], errors = [];
    const runDirectory = path.join(env.outputDirectory, 'vault-active-controls-'+Date.now());
    fs.mkdirSync(runDirectory, {recursive:true});
    async function open(seed = initial(), width = 1280, holdResumeNumber = 0) {
        const page = await browser.newPage({locale:'ru-RU',viewport:{width,height:850}});
        page.on('pageerror', error => errors.push(error.message));
        await page.setContent('<!doctype html><html><head><meta charset="UTF-8"><style>'+fs.readFileSync(path.join(env.root,'src/styles.css'),'utf8')+'</style></head><body><div id="root"></div></body></html>');
        await page.evaluate(({value,holdResumeNumber}) => {
            window.calls=[];window.queue=value;
            window.holdResumeNumber=holdResumeNumber;
            const current=value.items.find(item => item.id === value.activeId);
            window.savedTasks=current?.packageId ? [{id:current.packageId,assetId:current.assetId,title:current.title,kind:'Asset',selectedEngine:'5.8',path:current.requestedVaultPath+'/'+current.packageId,phase:'На паузе',done:current.progress.done,total:current.progress.total,filesDone:current.progress.filesDone,filesTotal:current.progress.filesTotal,intent:null}] : [];
        }, {value:seed,holdResumeNumber});
        await page.addScriptTag({content:bundle.outputFiles[0].text});
        await page.locator('.downloads-current .vault-queue-active[data-queue-id="a"]').waitFor();
        await page.locator('.vault-resume-tasks').waitFor({state:'hidden'});
        return page;
    }
    const active = page => page.locator('.downloads-current .vault-queue-active');
    const row = (page,id) => page.locator('.vault-queue-row[data-queue-id="'+id+'"]');
    const commands = page => page.evaluate(() => window.calls.filter(call => /^vault_queue_(pause_active|cancel_active|defer_active)$/.test(call.name)));
    async function order(page, ids) {
        assert.deepEqual(await page.locator('.vault-queue-row').evaluateAll(rows => rows.map(item => item.dataset.queueId)),ids);
    }
    async function nativeDrag(page, target) {
        const transfer = await page.evaluateHandle(() => new DataTransfer());
        await active(page).dispatchEvent('dragstart',{dataTransfer:transfer});
        const payload = await transfer.evaluate(value => value.getData('application/x-unreal-manager-active-queue'));
        await target.dispatchEvent('dragover',{dataTransfer:transfer});
        await target.dispatchEvent('drop',{dataTransfer:transfer});
        await active(page).dispatchEvent('dragend',{dataTransfer:transfer});
        await transfer.dispose();
        return payload ? JSON.parse(payload) : null;
    }
    async function pendingControl(page, kind) {
        await active(page).getByText(kind === 'defer' ? 'Возвращаем загрузку в очередь…' : 'Останавливаем загрузку…',{exact:true}).waitFor();
        assert.notEqual(await active(page).getAttribute('draggable'),'true');
        const buttons = active(page).getByRole('button');
        for (const button of await buttons.all()) assert(await button.isDisabled(),'Checkpoint wait disables repeated active controls');
        assert.equal(await active(page).locator('progress').first().getAttribute('value'),'37','Stopping keeps measured progress visible until checkpoint completion');
    }
    async function controlGeometry(page) {
        const geometry=await active(page).evaluate(card => {
            const rectangle=element => {const r=element.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
            return {viewport:innerWidth,card:rectangle(card),meter:rectangle(card.querySelector('.transfer-meter')),buttons:[...card.querySelectorAll('.vault-queue-active-actions button')].map(rectangle)};
        });
        const overlaps=(a,b) => a.left<b.right-1&&b.left<a.right-1&&a.top<b.bottom-1&&b.top<a.bottom-1;
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth>innerWidth+1),false,'Active controls do not cause horizontal overflow');
        assert.equal(geometry.buttons.length,3);
        for (const [index,button] of geometry.buttons.entries()) {
            assert(button.width>=44&&button.height>=44,'Active actions retain a usable click target');
            assert(button.left>=geometry.card.left-1&&button.right<=Math.min(geometry.card.right,geometry.viewport)+1,'Active action stays inside its card and viewport');
            assert(button.top>=geometry.card.top-1&&button.bottom<=geometry.card.bottom+1);
            assert(!overlaps(button,geometry.meter),'Active action does not overlap the progress meter');
            for (const other of geometry.buttons.slice(index+1)) assert(!overlaps(button,other),'Active actions do not overlap each other');
        }
        return geometry;
    }
    try {
        const pause = await open();
        await active(pause).getByRole('button',{name:'Пауза',exact:true}).click();
        await active(pause).getByText('UE 5.8 · На паузе',{exact:true}).waitFor();
        assert.equal(await active(pause).locator('progress').first().getAttribute('value'),'37');
        assert.deepEqual((await commands(pause)).at(-1),{name:'vault_queue_pause_active',args:{id:'a',paused:true,expectedRevision:21}});
        assert.equal(await pause.evaluate(() => window.queue.items[0].progress.done),3*1048576);
        assert.equal(await pause.evaluate(() => window.queue.items[0].progress.networkBytes),5*1048576);
        assert.equal(await pause.evaluate(() => window.queue.items[0].progress.bytesPerSecond),0);
        await active(pause).getByRole('button',{name:'Продолжить',exact:true}).click();
        await pause.waitForFunction(() => window.queue.items[0].progress.paused === false);
        assert.deepEqual((await commands(pause)).at(-1),{name:'vault_queue_pause_active',args:{id:'a',paused:false,expectedRevision:22}});
        checks.push('Queue pause/resume uses active ID and revision, preserving measured file/network progress');
        await pause.close();

        const before = await open();
        assert.equal(await active(before).getAttribute('draggable'),'true');
        const payload = await nativeDrag(before,row(before,'c'));
        assert.deepEqual(payload,{id:'a',revision:21});
        await pendingControl(before,'defer');
        assert.deepEqual((await commands(before)).at(-1),{name:'vault_queue_defer_active',args:{id:'a',beforeId:'c',expectedRevision:21}});
        await order(before,['b','c']);
        const count = (await commands(before)).length;
        await nativeDrag(before,row(before,'b'));
        assert.equal((await commands(before)).length,count,'Another drag cannot race the pending stop');
        await before.evaluate(value => window.deliverQueue(value),afterCheckpoint(['b','a','c'],'b'));
        await before.locator('.downloads-current .vault-queue-active[data-queue-id="b"]').waitFor();
        await order(before,['a','c']);
        await row(before,'a').getByText('UE 5.8 · В очереди · 37%',{exact:true}).waitFor();
        assert.equal(await before.locator('.vault-resume-tasks').count(),0,'A deferred checkpoint owned by the queue is not duplicated as a standalone resumable task');
        assert.equal(await before.locator('.downloads-current .vault-queue-active').count(),1);
        await before.evaluate(value => window.deliverQueue(value),afterCheckpoint(['a','c'],'a',24));
        await before.locator('.downloads-current .vault-queue-active[data-queue-id="a"]').waitFor();
        await order(before,['c']);
        checks.push('HTML5 active drag before a pending row retains the active card until native checkpoint, then displays B → A → C without duplicates');
        await before.close();

        const end = await open();
        await nativeDrag(end,end.locator('.vault-queue-drop-end[data-queue-drop-end]'));
        await pendingControl(end,'defer');
        assert.deepEqual((await commands(end)).at(-1),{name:'vault_queue_defer_active',args:{id:'a',beforeId:null,expectedRevision:21}});
        await end.evaluate(value => window.deliverQueue(value),afterCheckpoint(['b','c','a'],'b'));
        await end.locator('.downloads-current .vault-queue-active[data-queue-id="b"]').waitFor();
        await order(end,['c','a']);
        checks.push('Dropping an active task at the queue end sends beforeId=null and displays the presented B → C → A order');
        await end.close();

        const pointer = await open();
        const grip = pointer.locator('.vault-queue-active-grip');
        const start = await grip.boundingBox(), finish = await row(pointer,'c').boundingBox();
        assert(start && finish,'Active drag grip and pending target are visible');
        await pointer.mouse.move(start.x+start.width/2,start.y+start.height/2);
        await pointer.mouse.down();
        await pointer.mouse.move(finish.x+finish.width/2,finish.y+finish.height/2,{steps:8});
        await pointer.mouse.up();
        await pendingControl(pointer,'defer');
        assert.deepEqual((await commands(pointer)).at(-1),{name:'vault_queue_defer_active',args:{id:'a',beforeId:'c',expectedRevision:21}});
        checks.push('Pointer grip gesture targets the requested pending row with a scoped checkpoint command');
        await pointer.close();

        const button = await open(initial(),600);
        await active(button).getByRole('button',{name:'Вернуть в очередь',exact:true}).click();
        await pendingControl(button,'defer');
        assert.deepEqual((await commands(button)).at(-1),{name:'vault_queue_defer_active',args:{id:'a',beforeId:null,expectedRevision:21}});
        const dark600=await controlGeometry(button);
        await button.screenshot({path:path.join(runDirectory,'defer-wait-600.png'),animations:'disabled'});
        await button.setViewportSize({width:1280,height:850});
        await button.evaluate(() => {document.documentElement.dataset.theme='light';});
        const light1280=await controlGeometry(button);
        await button.screenshot({path:path.join(runDirectory,'defer-wait-light-1280.png'),animations:'disabled'});
        fs.writeFileSync(path.join(runDirectory,'defer-controls-layout.json'),JSON.stringify({dark600,light1280},null,2));
        checks.push('Explicit Return to queue uses the same end destination; dark 600px and light 1280px controls fit without overlapping the meter or each other');
        await button.close();

        // Deferral must yield to another task. A sole active task cannot defer,
        // and inserting before the first pending task would immediately restart it.
        const soleSeed=initial();soleSeed.items=soleSeed.items.slice(0,1);soleSeed.items[0].canDefer=false;
        const sole=await open(soleSeed);
        assert(await active(sole).getByRole('button',{name:'Вернуть в очередь',exact:true}).isDisabled());
        assert.notEqual(await active(sole).getAttribute('draggable'),'true');
        const soleTransfer=await sole.evaluateHandle(() => new DataTransfer());
        await active(sole).dispatchEvent('dragstart',{dataTransfer:soleTransfer});
        assert.equal(await soleTransfer.evaluate(value => value.getData('application/x-unreal-manager-active-queue')),'');
        assert.equal((await commands(sole)).length,0);
        await soleTransfer.dispose();await sole.close();
        checks.push('Sole active task disables deferral and cannot start an active drag');
        const first=await open();
        await nativeDrag(first,row(first,'b'));
        assert.equal((await commands(first)).length,0,'Dropping before the first pending item is a non-yielding move and sends no command');
        await order(first,['b','c']);
        assert.equal(await active(first).getAttribute('data-queue-id'),'a');
        await first.close();
        checks.push('A drop before the first pending item is rejected without changing the active task or queue');

        for (const kind of ['Asset','Project','Plugin']) {
            const seed = initial(); seed.items[0].title=kind+' fixture';
            const cancel = await open(seed);
            await active(cancel).getByRole('button',{name:'Отменить загрузку',exact:true}).click();
            await pendingControl(cancel,'cancel');
            assert.deepEqual((await commands(cancel)).at(-1),{name:'vault_queue_cancel_active',args:{id:'a',expectedRevision:21}});
            await active(cancel).getByRole('button').evaluateAll(buttons => buttons.forEach(item => item.click()));
            assert.equal((await commands(cancel)).length,1,'Pending cancel rejects repeated UI actions');
            await cancel.evaluate(value => {window.savedTasks=[];window.deliverQueue(value);},afterCheckpoint(['b','c'],'b'));
            await cancel.locator('.downloads-current .vault-queue-active[data-queue-id="b"]').waitFor();
            await order(cancel,['c']);
            assert.equal(await cancel.locator('[data-queue-id="a"]').count(),0);
            await cancel.locator('.vault-resume-tasks').waitFor({state:'hidden'});
            assert.equal(await cancel.evaluate(() => window.calls.filter(call => ['vault_pause','vault_cancel','vault_resume_forget','vault_queue_remove'].includes(call.name)).length),0,'Queue cancellation never invokes raw cancellation or deletes a saved task');
            await cancel.close();
        }
        checks.push('Asset, Project and Plugin active cancellation is ID/revision scoped, blocks repeats and presents the next task only after native completion');

        const late=await open(initial(),1280,2);
        await late.waitForFunction(() => typeof window.deliverOldResume === 'function');
        await active(late).getByRole('button',{name:'Отменить загрузку',exact:true}).click();
        await pendingControl(late,'cancel');
        await late.evaluate(value => {window.savedTasks=[];window.deliverQueue(value);},afterCheckpoint(['b','c'],'b'));
        await late.locator('.downloads-current .vault-queue-active[data-queue-id="b"]').waitFor();
        await late.locator('.vault-resume-tasks').waitFor({state:'hidden'});
        await late.waitForFunction(() => window.resumeReads >= 3);
        await late.evaluate(async () => {window.deliverOldResume();await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));});
        assert.equal(await late.locator('.vault-resume-tasks').count(),0,'An older task-list response must not resurrect the removed checkpoint after cancellation');
        await late.close();
        checks.push('An out-of-order saved-task response cannot resurrect a cancelled checkpoint after the authoritative empty refresh');

        const stale = await open();
        const transfer = await stale.evaluateHandle(() => new DataTransfer());
        await active(stale).dispatchEvent('dragstart',{dataTransfer:transfer});
        const updated = initial(); updated.revision=22;
        await stale.evaluate(value => window.deliverQueue(value),updated);
        await row(stale,'c').dispatchEvent('dragover',{dataTransfer:transfer});
        await row(stale,'c').dispatchEvent('drop',{dataTransfer:transfer});
        await stale.getByText(/Очередь изменилась\. Повторите действие\./).waitFor();
        assert.deepEqual((await commands(stale)).at(-1),{name:'vault_queue_defer_active',args:{id:'a',beforeId:'c',expectedRevision:21}},'Drag retains its captured revision instead of silently reinterpreting a newer snapshot');
        await order(stale,['b','c']);
        await transfer.dispose();
        checks.push('A drag captured before a revision change is refused and refreshed without an optimistic queue reorder');
        await stale.close();
        assert.deepEqual(errors,[],'Renderer did not throw');
        fs.writeFileSync(path.join(runDirectory,'report.json'),JSON.stringify({passed:true,scope:'Real renderer/useVault with synthetic IPC snapshots; no real download, checkpoint or installation acceptance claimed',checks},null,2));
        console.log('PASS active queue controls: '+checks.length+' renderer scenarios; evidence '+runDirectory);
    } finally { await browser.close(); }
})().catch(error => {console.error(error);process.exitCode=1;});
