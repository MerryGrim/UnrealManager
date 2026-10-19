import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { ExternalLink, X } from 'lucide-react';
import metadata from '../package.json';
type License = {
    name: string;
    version: string;
    license: string;
    source: string;
    texts: {
        name: string;
        text: string;
    }[];
};
export function AboutDialog({ onClose }: {
    onClose: () => void;
}) {
    const [tab, setTab] = useState('app'), [licenses, setLicenses] = useState<License[]>([]), [query, setQuery] = useState(''), [error, setError] = useState('');
    const close = useRef<HTMLButtonElement>(null);
    useEffect(() => { close.current?.focus(); void import('./licenses.json').then(data => setLicenses(data.default)); }, []);
    return createPortal(<div className="modal-backdrop" onKeyDown={e => { if (e.key === 'Escape')
        onClose(); if (e.key === 'Tab') {
        const list = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled),input,summary'));
        const i = list.indexOf(document.activeElement as HTMLElement);
        if (e.shiftKey && i <= 0) {
            e.preventDefault();
            list.at(-1)?.focus();
        }
        else if (!e.shiftKey && i === list.length - 1) {
            e.preventDefault();
            list[0]?.focus();
        }
    } }}><section className="about-dialog" role="dialog" aria-modal="true" aria-label={tr("О приложении")}><div className="modal-head"><h2>{tr("О приложении")}</h2><button ref={close} className="icon" aria-label={tr("Закрыть")} onClick={onClose}><X size={20}/></button></div><div className="tabs" role="tablist" aria-label={tr("Сведения о приложении")}>{tr([['app', 'Приложение'], ['licenses', 'Лицензии и копирайты'], ['access', 'Доступ и данные']].map(([id, label]) => <button key={id} id={`about-tab-${id}`} role="tab" aria-selected={tab === id} aria-controls={`about-panel-${id}`} className={tab === id ? 'selected' : ''} onClick={() => setTab(id)}>{tr(label)}</button>))}</div><div className="about-body" id={`about-panel-${tab}`} role="tabpanel" aria-labelledby={`about-tab-${tab}`}>
 {tr(tab === 'app' && <><h3>{tr("Unreal Manager")}</h3><p>{tr("Версия ")}<strong>{metadata.version}</strong></p><p>{tr("Управление движками Unreal Engine, проектами, материалами Fab и плагинами.")}</p><dl><dt>{tr("Автор")}</dt><dd>{tr("Nyanma")}</dd></dl><button onClick={() => void invoke('open_author_profile').catch(e => setError(String(e)))}><ExternalLink size={17}/>{tr("github.com/MerryGrim")}</button><p>{tr("© 2026 Nyanma. Исходный код приложения распространяется по лицензии MIT.")}</p><p>{tr("Unreal Engine, Epic Games и Fab — обозначения их правообладателей. Unreal Manager — независимый проект.")}</p></>)}
 {tr(tab === 'licenses' && <><p>{tr("Сторонние компоненты и их лицензии. Оригинальные тексты сохранены без перевода.")}</p><input className="license-search" placeholder={tr("Поиск компонента")} value={query} onChange={e => setQuery(e.target.value)}/><div className="license-list">{tr(licenses.filter(x => `${x.name} ${x.license}`.toLowerCase().includes(query.toLowerCase())).map(x => <details key={`${x.source}:${x.name}:${x.version}`}><summary>{x.name} {x.version}<small>{tr(x.license)}</small></summary>{tr(x.texts.length ? x.texts.map((text, i) => <div key={i}><b>{text.name}</b><pre translate="no">{text.text}</pre></div>) : <p>{tr("Текст лицензии отсутствует в установленном пакете; идентификатор взят из его метаданных.")}</p>)}</details>))}</div></>)}
 {tr(tab === 'access' && <><h3>{tr("Локальные файлы")}</h3><p>{tr("Приложение читает каталоги движков, проектов, Vault и кэш. По вашему действию скачивает, устанавливает, обновляет, копирует и удаляет файлы в выбранных папках.")}</p><h3>{tr("Аккаунт и сеть")}</h3><p>{tr("Подключение к Epic Games и Fab используется для входа, библиотеки, активации ключей и загрузок. Пароль и двухфакторный код вводятся на официальном сайте Epic.")}</p><h3>{tr("Сохранённые данные")}</h3><p>{tr("Сеанс Epic защищён средствами Windows. Каталог, обложки, настройки языка и история загрузок сохраняются локально. Для перевода интерфейса сеть не нужна.")}</p><h3>{tr("Внешние программы")}</h3><p>{tr("По вашему действию открываются Unreal Editor, Проводник и браузер. Установка и удаление движков затрагивают их локальные записи Epic.")}</p><p>{tr("Этот раздел описывает поведение программы и не заменяет разрешения Windows.")}</p></>)}
 {tr(error && <p className="error-text" role="alert">{tr(error)}</p>)}</div></section></div>, document.body);
}
