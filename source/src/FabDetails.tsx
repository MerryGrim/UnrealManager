import { tr, getLocale } from './i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, ChevronLeft, ChevronRight, Download, ExternalLink, RefreshCw, Maximize2, X } from 'lucide-react';
import { FabCover, type FabItem } from './EpicLibrary';
type Details = {
    id: string;
    description: string;
    author: string;
    images: string[];
    url: string | null;
    cachedAt: number;
    notice: string;
};
// HTML from the publisher becomes plain text; no remote markup, links or scripts enter the app DOM.
export function descriptionText(html: string) { const d = new DOMParser().parseFromString(html, 'text/html'); d.querySelectorAll('script,style,iframe,object,template').forEach(n => n.remove()); d.querySelectorAll('p,div,li,h1,h2,h3,h4,br,tr').forEach(n => { n.prepend(d.createTextNode(n.tagName === 'LI' ? '\n• ' : '\n')); }); return (d.body.textContent || '').replace(/\n[ \t]+/g, '\n').replace(/\n{3,}/g, '\n\n').trim(); }
export function FabDetails({ item, busy, onClose, onDownload }: {
    item: FabItem;
    busy: boolean;
    onClose: () => void;
    onDownload: (item: FabItem, install?: boolean, downloadOnly?: boolean) => void;
}) {
    const [info, setInfo] = useState<Details | null>(null), [loading, setLoading] = useState(true), [error, setError] = useState(''), [revision, setRevision] = useState(0), [index, setIndex] = useState(0), [src, setSrc] = useState(''), [imageError, setImageError] = useState(''), [expanded, setExpanded] = useState(false);
    const back = useRef<HTMLButtonElement>(null), panel = useRef<HTMLElement>(null);
    useEffect(() => { back.current?.focus(); }, []);
    useEffect(() => { let live = true; setLoading(true); setError(''); invoke<Details>('fab_details', { id: item.id, refresh: revision > 0 }).then(d => { if (live) {
        setInfo(d);
        setIndex(0);
    } }).catch(e => { if (live)
        setError(String(e)); }).finally(() => { if (live)
        setLoading(false); }); return () => { live = false; }; }, [item.id, revision]);
    useEffect(() => { let live = true; setSrc(''); setImageError(''); if (!info?.images.length)
        return; invoke<string>('fab_detail_image', { id: item.id, index }).then(s => { if (live)
        setSrc(s); }).catch(e => { if (live)
        setImageError(String(e)); }); return () => { live = false; }; }, [item.id, info, index]);
    const move = (delta: number) => setIndex(i => (i + delta + (info?.images.length || 1)) % (info?.images.length || 1));
    const openSite = () => void invoke('fab_open_listing', { id: item.id }).catch(e => setError(String(e)));
    return createPortal(<div className="fab-info-backdrop" onKeyDown={e => {
            if (e.key === 'Escape') {
                e.preventDefault();
                if (expanded)
                    setExpanded(false);
                else
                    onClose();
            }
            if (e.key === 'Tab') {
                const nodes = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],input,select,[tabindex="0"]') || []).filter(n => n.getClientRects().length);
                const i = nodes.indexOf(document.activeElement as HTMLElement);
                if (e.shiftKey && i <= 0) {
                    e.preventDefault();
                    nodes.at(-1)?.focus();
                }
                else if (!e.shiftKey && (i < 0 || i === nodes.length - 1)) {
                    e.preventDefault();
                    nodes[0]?.focus();
                }
            }
        }}><section ref={panel} className="fab-info-page" role="dialog" aria-modal="true" aria-label={tr(`Информация: ${item.title}`)}>
  <header className="fab-info-header"><button ref={back} onClick={onClose}><ArrowLeft size={18}/>{tr("К библиотеке")}</button><button disabled={loading} onClick={() => setRevision(r => r + 1)}><RefreshCw size={17} className={loading ? 'spin' : ''}/>{tr("Обновить информацию")}</button></header>
  <div className="fab-info-scroll"><div className="fab-info-heading"><FabCover item={item}/><div><span className="eyebrow">{tr(item.kind === 'Plugin' ? 'ПЛАГИН' : item.kind === 'Project' ? 'ПРОЕКТ' : 'АССЕТ')}</span><h2>{item.title}</h2>{tr(info?.author && <p>{info.author}</p>)}<p className="muted">{tr(item.categories.join(' · '))}</p></div></div>
   {tr(loading && <p className="fab-info-loading" role="status"><RefreshCw className="spin" size={18}/>{tr("Загружаем информацию…")}</p>)}
   {tr(error && <p className="error-text" role="alert">{tr(error)}</p>)}{tr(info?.notice && <p className="muted" role="status">{tr(info.notice)}</p>)}
   <div className="fab-info-columns"><div>
    {tr(!!info?.images.length && <div className={`fab-info-gallery ${expanded ? 'expanded' : ''}`}>
      <div className="fab-info-image">{tr(src ? <img src={src} alt={tr(`${item.title} — изображение ${index + 1}`)}/> : <p role="status">{tr(imageError || 'Загружаем изображение…')}</p>)}</div>
      <div className="fab-info-gallery-actions"><button aria-label={tr("Предыдущее изображение")} disabled={info.images.length < 2} onClick={() => move(-1)}><ChevronLeft size={19}/></button><span>{tr(index + 1)}{tr(" / ")}{tr(info.images.length)}</span><button aria-label={tr("Следующее изображение")} disabled={info.images.length < 2} onClick={() => move(1)}><ChevronRight size={19}/></button><button onClick={() => setExpanded(v => !v)}>{tr(expanded ? <X size={18}/> : <Maximize2 size={18}/>)} {tr(expanded ? 'Закрыть увеличение' : 'Увеличить')}</button></div>
    </div>)}
    <h3>{tr("Описание")}</h3><div className="fab-info-description">{descriptionText(info?.description || item.description || '') || tr('Автор не предоставил описание.')}</div>
   </div><aside><h3>{tr("Совместимость")}</h3><div className="fab-info-versions">{tr(item.engineVersions.length ? item.engineVersions.map(v => <span className="engine-pill" key={v}>{tr("UE ")}{tr(v)}</span>) : <p className="muted">{tr("Версии не указаны")}</p>)}</div><h3>{tr("Платформы")}</h3><p>{tr([...new Set(item.versions.flatMap(v => v.platforms))].join(', ') || 'Не указаны')}</p>
    <button className="primary" onClick={() => { onClose(); onDownload(item, false, true); }}><Download size={17}/>{tr("Скачать в хранилище")}</button>
    {tr(item.kind !== 'Plugin' && <button disabled={busy} onClick={() => { onClose(); onDownload(item, true); }}>{tr(item.kind === 'Project' ? 'Создать проект' : 'Добавить в проект')}</button>)}
    {tr(info?.url && <button onClick={openSite}><ExternalLink size={17}/>{tr("Открыть на Fab")}</button>)}
    {tr(!!info?.cachedAt && <p className="muted">{tr("Информация сохранена ")}{tr(new Date(info.cachedAt * 1000).toLocaleString(getLocale()))}</p>)}
   </aside></div>
  </div>
 </section></div>, document.body);
}
