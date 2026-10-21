import {useLayoutEffect,useRef,type ReactNode} from 'react';

const controls='button,input:not([type="hidden"]),select,textarea,a[href],summary,[tabindex],[contenteditable="true"]';
const visible=(element:HTMLElement)=>element.isConnected&&!element.closest('[hidden],[inert]')&&element.getClientRects().length>0&&getComputedStyle(element).visibility!=='hidden';
const available=(element:HTMLElement)=>visible(element)&&!element.matches(':disabled,[aria-disabled="true"]');
function topDialog(){return Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"],[role="alertdialog"]')).filter(visible).at(-1);}
function focusable(dialog:HTMLElement){
    return Array.from(dialog.querySelectorAll<HTMLElement>(controls)).filter(element=>available(element)&&element.tabIndex>=0).filter(element=>{
        if(!(element instanceof HTMLInputElement)||element.type!=='radio'||!element.name)return true;
        const group=Array.from(dialog.querySelectorAll<HTMLInputElement>('input[type="radio"]')).filter(radio=>radio.name===element.name&&available(radio));
        return element===(group.find(radio=>radio.checked)||group[0]);
    });
}

/** Event-driven keyboard handling; only the uppermost visible dialog owns focus. */
export function ModalFocusScope({children,onClose,closeDisabled=false,className='modal-backdrop'}:{children:ReactNode;onClose:()=>void;closeDisabled?:boolean;className?:string}){
    const root=useRef<HTMLDivElement>(null),latest=useRef({onClose,closeDisabled});
    latest.current={onClose,closeDisabled};
    useLayoutEffect(()=>{
        const scope=root.current,dialog=scope?.querySelector<HTMLElement>('[role="dialog"],[role="alertdialog"]');
        if(!scope||!dialog)return;
        const opener=document.activeElement instanceof HTMLElement?document.activeElement:null;
        if(!dialog.hasAttribute('tabindex'))dialog.tabIndex=-1;
        const active=()=>{const top=topDialog();return !!top&&scope.contains(top);};
        const first=()=>{
            const preferred=dialog.querySelector<HTMLElement>('[data-modal-initial-focus],[autofocus]');
            (preferred&&available(preferred)?preferred:focusable(dialog)[0]||dialog).focus({preventScroll:true});
        };
        const keydown=(event:KeyboardEvent)=>{
            if(!active()||event.defaultPrevented||event.isComposing)return;
            if(event.key==='Escape'){
                event.preventDefault();event.stopPropagation();
                if(!latest.current.closeDisabled)latest.current.onClose();
            }else if(event.key==='Tab'){
                const list=focusable(dialog),index=list.indexOf(document.activeElement as HTMLElement);
                event.preventDefault();
                if(!list.length)dialog.focus({preventScroll:true});
                else list[index<0?(event.shiftKey?list.length-1:0):(index+(event.shiftKey?list.length-1:1))%list.length].focus();
            }
        };
        const focusin=(event:FocusEvent)=>{if(active()&&!scope.contains(event.target as Node))first();};
        document.addEventListener('keydown',keydown);
        document.addEventListener('focusin',focusin);
        if(active())first();
        return()=>{
            document.removeEventListener('keydown',keydown);
            document.removeEventListener('focusin',focusin);
            const top=topDialog();
            if(opener&&available(opener)&&(!top||scope.contains(top)||top.contains(opener)))opener.focus({preventScroll:true});
        };
    },[]);
    return <div ref={root} className={className}>{children}</div>;
}
