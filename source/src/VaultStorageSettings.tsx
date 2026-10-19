import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {FolderOpen,Save} from 'lucide-react';
import {tr} from './i18n';
type Options={vaultPath:string;keepEngineBackup:boolean};
export function VaultStorageSettings({busy,onChanged}:{busy:boolean;onChanged:()=>Promise<void>}){
 const [options,setOptions]=useState<Options|null>(null),[saving,setSaving]=useState(false),[message,setMessage]=useState(''),[error,setError]=useState('');
 useEffect(()=>{let active=true;invoke<Options>('vault_storage_get').then(o=>{if(active)setOptions(o);}).catch(e=>{if(active)setError(String(e));});return()=>{active=false;};},[]);
 async function choose(){try{const path=await invoke<string|null>('choose_project_location',{folder:true});if(path)setOptions(o=>o?{...o,vaultPath:path}:o);}catch(e){setError(String(e));}}
 async function save(){if(!options||busy||saving)return;setSaving(true);setError('');setMessage('');try{await invoke('vault_storage_set',{path:options.vaultPath,keepEngineBackup:options.keepEngineBackup});await onChanged();setMessage('Настройки хранилища сохранены.');}catch(e){setError(String(e));}finally{setSaving(false);}}
 return <section><span className="eyebrow">{tr('ХРАНИЛИЩЕ')}</span><h3>{tr('Хранилище Unreal Manager')}</h3><p className="muted">{tr('Новые материалы скачиваются в выбранную папку. При смене пути существующие файлы остаются на прежнем месте; верните прежний путь, чтобы снова увидеть их.')}</p>
 {options&&<><label htmlFor="um-vault-path">{tr('Папка хранилища')}</label><div className="epic-vault-controls"><input id="um-vault-path" value={options.vaultPath} disabled={busy||saving} onChange={e=>setOptions({...options,vaultPath:e.target.value})}/><button disabled={busy||saving} onClick={choose}><FolderOpen size={17}/>{tr('Выбрать папку')}</button></div><label className="filter-check"><input type="checkbox" checked={options.keepEngineBackup} disabled={busy||saving} onChange={e=>setOptions({...options,keepEngineBackup:e.target.checked})}/>{tr('Сохранять прежние файлы движка для отката')}</label><p className="muted">{tr('После успешной установки временные файлы удаляются автоматически. Резервная копия для отката может занимать много места.')}</p><button disabled={busy||saving||!options.vaultPath.trim()} onClick={save}><Save size={17}/>{tr(saving?'Сохраняем…':'Сохранить')}</button></>}
 {message&&<p role="status">{tr(message)}</p>}{error&&<p role="alert">{tr(error)}</p>}</section>;
}
