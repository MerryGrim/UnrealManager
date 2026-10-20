import {tr,getLocale} from './i18n';
import {ChevronRight} from 'lucide-react';
import logo from './assets/um-logo.svg';
import {EnginePauseButton} from './EnginePauseButton';
export type EngineProgress = {phase:string;done:number;total:number;percent:number;processedBytes?:number;totalBytes?:number;filesDone?:number;filesTotal?:number;processingBytesPerSecond?:number;currentFile?:string|null;filesPerSecond?:number;readBytesPerSecond?:number;paused?:boolean;canPause?:boolean};
export type EngineTransfer = EngineProgress & {networkBytes:number;bytesPerSecond:number;title:string};
export const transferSize=(bytes:number)=>`${(bytes/1048576).toLocaleString(getLocale(),{maximumFractionDigits:1})} ${tr('МБ')}`;
export const formatEnginePhase=(phase:string)=>phase.startsWith('Fab: ')?`Fab: ${tr(phase.slice(5))}`:tr(phase);
const countLabels:Record<string,string>={
 'Проверяем сохранённую задачу':'Записи',
 'Проверяем наличие блоков':'Блоки',
 'Проверяем сохранённые блоки':'Блоки',
 'Считаем временные файлы':'Объекты',
 'Удаляем временные файлы':'Объекты',
 'Завершаем очистку':'Объекты',
 'Считаем файлы движка':'Объекты',
 'Удаляем файлы движка':'Объекты',
 'Удаляем записи движка':'Объекты',
 'Проверяем системные компоненты':'Компоненты',
 'Ожидаем завершения установки системных компонентов':'Компоненты',
 'Устанавливаем системные компоненты':'Компоненты',
};
const childPhase=(phase:string)=>phase.startsWith('Fab: ')?phase.slice(5):phase;
export const engineCountLabel=(phase:string)=>countLabels[childPhase(phase)]||'Файлы';
export const isEngineObjectInventory=(phase:string)=>['Считаем временные файлы','Считаем файлы движка'].includes(childPhase(phase));
export function engineProgressPercent(progress:EngineProgress):number|undefined {
 if(isEngineObjectInventory(progress.phase)||!Number.isFinite(progress.done)||!Number.isFinite(progress.total)||progress.total<=0||progress.done<0)return undefined;
 return Math.min(100,progress.done/progress.total*100);
}
export function enginePercentText(progress:EngineProgress):string {
 const percent=engineProgressPercent(progress);
 if(percent===undefined)return '—';
 const format=(value:number)=>value.toLocaleString(getLocale(),{maximumFractionDigits:1});
 if(percent>0&&percent<0.1)return `<${format(0.1)}%`;
 // Rounded display must not claim completion before the last item is checked.
 return `${format(percent<100?Math.min(99.9,percent):100)}%`;
}
const count=(value:number)=>value.toLocaleString(getLocale(),{maximumFractionDigits:0});
export function engineProgressMetrics(progress:EngineProgress):{label:string;value:string;currentFile?:boolean}[] {
 const rows:{label:string;value:string;currentFile?:boolean}[]=[];
 if(progress.filesTotal)rows.push({label:engineCountLabel(progress.phase),value:`${count(progress.filesDone||0)} / ${count(progress.filesTotal)}`});
 else if(isEngineObjectInventory(progress.phase))rows.push({label:'Найдено объектов',value:count(progress.filesDone||0)});
 if(progress.totalBytes) {
  rows.push({label:'Обработано',value:`${transferSize(progress.processedBytes||0)} / ${transferSize(progress.totalBytes)}`});
  rows.push({label:'Скорость обработки',value:`${transferSize(progress.processingBytesPerSecond||0)}${tr('/с')}`});
 } else if(progress.readBytesPerSecond!==undefined)rows.push({label:'Прочитано',value:transferSize(progress.processedBytes||0)});
 if(Number.isFinite(progress.filesPerSecond))rows.push({label:'Скорость проверки',value:`${Math.max(0,progress.filesPerSecond!).toLocaleString(getLocale(),{maximumFractionDigits:1})} ${tr('файл/с')}`});
 if(Number.isFinite(progress.readBytesPerSecond))rows.push({label:'Скорость чтения',value:`${transferSize(Math.max(0,progress.readBytesPerSecond!))}${tr('/с')}`});
 if(progress.currentFile)rows.push({label:'Текущий файл',value:progress.currentFile,currentFile:true});
 return rows;
}
export function EngineTransferCard({progress,onOpen}:{progress:EngineTransfer;onOpen:()=>void}){
 return <article className="transfer-card engine-transfer-card"><div className="transfer-main"><div className="transfer-art"><img src={logo} alt=""/></div><div className="transfer-info"><h3>{progress.title}</h3><p>{progress.paused?tr('На паузе'):formatEnginePhase(progress.phase)}</p><div className="transfer-meter"><progress aria-label={tr('Прогресс загрузки')} aria-valuetext={enginePercentText(progress)} max={100} value={engineProgressPercent(progress)}/><strong>{enginePercentText(progress)}</strong></div></div><div className="transfer-buttons"><EnginePauseButton paused={progress.paused} canPause={progress.canPause}/><button onClick={onOpen}>{tr('Открыть установку движка')}<ChevronRight size={17}/></button></div></div><div className="transfer-metrics"><div><small>{tr('Скорость сети')}</small><b>{transferSize(progress.bytesPerSecond)}{tr('/с')}</b></div><div><small>{tr('Получено по сети')}</small><b>{transferSize(progress.networkBytes)}</b></div><div><small>{tr('Состояние')}</small><b>{progress.paused?tr('На паузе'):formatEnginePhase(progress.phase)}</b></div>{engineProgressMetrics(progress).map(row=><div key={row.label} className={row.currentFile?'engine-current-file':undefined}><small>{tr(row.label)}</small><b title={row.currentFile?row.value:undefined}>{row.value}</b></div>)}</div></article>;
}
