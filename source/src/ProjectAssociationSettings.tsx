import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {tr} from './i18n';
type Status={enabled:boolean;windowsChoiceRequired:boolean};
export function ProjectAssociationSettings(){
 const [status,setStatus]=useState<Status|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
 useEffect(()=>{let active=true;invoke<Status>('project_association_get').then(s=>{if(active)setStatus(s);}).catch(e=>{if(active)setError(String(e));});return()=>{active=false;};},[]);
 async function change(enabled:boolean){setBusy(true);setError('');try{setStatus(await invoke<Status>('project_association_set',{enabled}));}catch(e){setError(String(e));}finally{setBusy(false);}}
 async function repair(){setBusy(true);setError('');setMessage('');try{await invoke('engine_register_managed');setMessage('Регистрация движков проверена.');}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <section><span className="eyebrow">{tr('ПРОЕКТЫ')}</span><h3>{tr('Открытие проектов из Проводника')}</h3><label className="filter-check"><input type="checkbox" disabled={busy||!status} checked={status?.enabled||false} onChange={e=>void change(e.target.checked)}/>{tr('Открывать .uproject в последней установленной версии Unreal Engine')}</label><p className="muted">{tr('Версия определяется при каждом открытии. Незавершённые установки пропускаются. Unreal Manager не переписывает .uproject. Новая версия движка может потребовать обновления проекта.')}</p>{status?.enabled&&status.windowsChoiceRequired&&<p role="status">{tr('Windows уже выбрала другую программу для .uproject. В меню «Открыть с помощью» выберите Unreal Manager и используйте его по умолчанию.')}</p>}<button disabled={busy} onClick={repair}>{tr('Проверить регистрацию движков')}</button>{message&&<p role="status">{tr(message)}</p>}{error&&<p role="alert">{tr(error)}</p>}</section>;
}
