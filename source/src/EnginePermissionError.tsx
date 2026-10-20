import {tr} from './i18n';

const prefix='UM_ELEVATION_REQUIRED:';
const operationLabels={epicRecord:'Запись Epic Games',engineFiles:'Файлы движка',staging:'Временные файлы установки',recovery:'Восстановление установки',deleteEngine:'Удаление движка'} as const;
type PermissionIssue={operation:keyof typeof operationLabels;path:string;message:string;details?:string};

// Malformed native envelopes must not expose internal JSON in the interface.
export function parseEnginePermissionError(error:string):PermissionIssue|null {
    if(!error.startsWith(prefix)||error.length>65536)return null;
    try {
        const value:unknown=JSON.parse(error.slice(prefix.length));
        if(!value||typeof value!=='object'||Array.isArray(value))return null;
        const record=value as Record<string,unknown>;
        if(typeof record.operation!=='string'||!Object.prototype.hasOwnProperty.call(operationLabels,record.operation)||typeof record.path!=='string'||!record.path.trim()||typeof record.message!=='string'||!record.message.trim()||record.message.includes(prefix)||record.path.includes(prefix))return null;
        if(record.details!==undefined&&(typeof record.details!=='string'||record.details.length>16384||record.details.includes(prefix)))return null;
        return {operation:record.operation as PermissionIssue['operation'],path:record.path,message:record.message,details:record.details as string|undefined};
    }catch{return null;}
}

export function EnginePermissionError({error}:{error:string}) {
    const issue=parseEnginePermissionError(error);
    const context=/^Ошибка файловой системы \(([^)]+)\):/.exec(error);
    const operation=context?.[1];
    const label=operation&&Object.prototype.hasOwnProperty.call(operationLabels,operation)?operationLabels[operation as keyof typeof operationLabels]:operation;
    const plain=operation?error.replace(`(${operation})`,`(${tr(label!)})`):error;
    if(!issue)return <p className="error-text" role="alert">{tr(error.includes(prefix)?'Не удалось прочитать сведения об ошибке доступа. Повторите проверку.':plain)}</p>;
    return <div className="error-text engine-permission-error" role="alert">
        <p>{tr(issue.message)}</p>
        <p><strong>{tr(operationLabels[issue.operation])}</strong></p>
        <p className="engine-permission-path">{issue.path}</p>
        {issue.details&&<p>{tr('Подробности ошибки')}: <span className="engine-permission-path">{issue.details}</span></p>}
        <p>{tr(issue.operation==='epicRecord'?'Проверьте права доступа к указанной записи Epic и повторите операцию.':'Выберите папку движка, доступную для записи, или исправьте права доступа к указанному файлу.')}</p>
    </div>;
}
