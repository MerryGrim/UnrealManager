const textInputTypes = new Set(['text', 'search', 'email', 'password', 'url', 'tel', 'number']);

export function installMainContextMenuPolicy(doc: Document = document): () => void {
    const handle = (event: MouseEvent) => {
        if (event.defaultPrevented) return;
        const target = event.target instanceof Element ? event.target : null;
        const field = target?.closest('input,textarea');
        if (field instanceof HTMLTextAreaElement && !field.disabled) return;
        if (field instanceof HTMLInputElement && !field.disabled && textInputTypes.has(field.type)) return;
        if (target instanceof HTMLElement && target.isContentEditable) return;

        const selection = doc.getSelection();
        if (selection && !selection.isCollapsed && selection.toString()) {
            for (let index = 0; index < selection.rangeCount; index++) {
                for (const rect of selection.getRangeAt(index).getClientRects()) {
                    if (rect.width > 0 && rect.height > 0 && event.clientX >= rect.left && event.clientX <= rect.right
                        && event.clientY >= rect.top && event.clientY <= rect.bottom) return;
                }
            }
        }
        event.preventDefault();
    };
    doc.addEventListener('contextmenu', handle);
    return () => doc.removeEventListener('contextmenu', handle);
}
