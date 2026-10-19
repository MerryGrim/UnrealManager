import {Minimize2,Power} from 'lucide-react';
import {useEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {tr} from './i18n';
export function ClosePrompt(){
 const [open,setOpen]=useState(false),[remember,setRemember]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const cancel=useRef<HTMLButtonElement>(null),lock=useRef(false);
 useEffect(()=>{let disposed=false;const subscription=listen<string>('um-close-request',event=>{if(!disposed){setError(event.payload==='__closing__'?'':event.payload||'');setBusy(event.payload==='__closing__');setRemember(false);setOpen(true);}});return()=>{disposed=true;void subscription.then(unlisten=>unlisten());};},[]);
 useEffect(()=>{if(open)cancel.current?.focus();},[open]);
 async function choose(action:'tray'|'exit'){if(lock.current)return;lock.current=true;setBusy(true);setError('');try{await invoke('window_close_action',{action,remember});setOpen(false);}catch(e){setError(String(e));}finally{lock.current=false;setBusy(false);}}
 if(!open)return null;
 return createPortal(<div className="modal-backdrop close-prompt-backdrop" onKeyDown={e=>{if(e.key==='Escape'&&!busy)setOpen(false);if(e.key==='Tab'){const controls=Array.from(e.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled)'));const index=controls.indexOf(document.activeElement as HTMLElement);e.preventDefault();controls[(index+(e.shiftKey?controls.length-1:1))%controls.length]?.focus();}}}><section className="engine-wizard close-prompt" role="dialog" aria-modal="true" aria-label={tr('Закрыть приложение?')}><h2>{tr('Закрыть приложение?')}</h2><p>{tr('В трее приложение продолжит загрузки. При закрытии загрузки сохранятся для продолжения после запуска.')}</p><label className="downloaded-filter"><input type="checkbox" checked={remember} disabled={busy} onChange={e=>setRemember(e.target.checked)}/>{tr('Больше не спрашивать')}</label><p className="muted">{tr('Выбор можно изменить в настройках.')}</p>{busy&&<p className="muted" role="status">{tr("Останавливаем задачи и сохраняем состояние…")}</p>}{error&&<p role="alert" className="error-text">{tr(error)}</p>}<div className="wizard-footer"><button ref={cancel} disabled={busy} onClick={()=>setOpen(false)}>{tr('Отмена')}</button><button className="primary" disabled={busy} onClick={()=>void choose('tray')}><Minimize2 size={16}/>{tr('Свернуть в трей')}</button><button disabled={busy} onClick={()=>void choose('exit')}><Power size={16}/>{tr('Закрыть приложение')}</button></div></section></div>,document.body);
}
export function WindowSettings(){
 const [value,setValue]=useState('ask'),[busy,setBusy]=useState(true),[error,setError]=useState('');
 useEffect(()=>{let active=true;invoke<string>('close_behavior_get').then(v=>{if(active)setValue(v);}).catch(e=>{if(active)setError(String(e));}).finally(()=>{if(active)setBusy(false);});return()=>{active=false;};},[]);
 async function change(behavior:string){setBusy(true);setError('');try{await invoke('close_behavior_set',{behavior});setValue(behavior);}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <section><h3>{tr('Поведение окна')}</h3><div className="setting-row"><label htmlFor="close-behavior"><b>{tr('При нажатии крестика')}</b></label><select id="close-behavior" disabled={busy} value={value} onChange={e=>void change(e.target.value)}><option value="ask">{tr('Всегда спрашивать')}</option><option value="tray">{tr('Свернуть в трей')}</option><option value="exit">{tr('Закрыть приложение')}</option></select></div>{busy&&<p className="muted" role="status">{tr("Останавливаем задачи и сохраняем состояние…")}</p>}{error&&<p role="alert" className="error-text">{tr(error)}</p>}</section>;
}
