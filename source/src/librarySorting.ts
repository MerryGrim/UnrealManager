export const librarySortOptions = ['title-asc', 'title-desc', 'kind', 'engine-desc', 'engine-asc', 'downloaded', 'not-downloaded'] as const;
export type LibrarySort = typeof librarySortOptions[number];
type SortableItem = { id: string; title: string; kind: string; engineVersions: string[] };

export function isLibrarySort(value: unknown): value is LibrarySort {
    return librarySortOptions.includes(value as LibrarySort);
}

export function sortLibraryItems<T extends SortableItem>(items: readonly T[], sort: LibrarySort, locale: string,
    downloaded: ReadonlySet<string>, kindLabel: (kind: string) => string): T[] {
    const compare = new Intl.Collator(locale, { numeric: true, sensitivity: 'base' }).compare;
    const byName = (a: T, b: T) => compare(a.title, b.title) || compare(a.id, b.id);
    const newest = (item: T) => item.engineVersions.map(version => version.match(/^(?:UE[_\s-]*)?(\d+\.\d+(?:\.\d+)?)/i)?.[1])
        .filter((version): version is string => !!version).sort(compare).at(-1);
    const engines = sort.startsWith('engine-') ? new Map(items.map(item => [item.id, newest(item)])) : null;
    return [...items].sort((a, b) => {
        if (sort === 'title-desc') return compare(b.title, a.title) || compare(a.id, b.id);
        if (sort === 'kind') return compare(kindLabel(a.kind), kindLabel(b.kind)) || byName(a, b);
        if (engines) {
            const av = engines.get(a.id), bv = engines.get(b.id);
            if (!av || !bv) return av ? -1 : bv ? 1 : byName(a, b);
            return (sort === 'engine-desc' ? compare(bv, av) : compare(av, bv)) || byName(a, b);
        }
        if (sort === 'downloaded' || sort === 'not-downloaded') {
            const order = Number(downloaded.has(b.id)) - Number(downloaded.has(a.id));
            return (sort === 'downloaded' ? order : -order) || byName(a, b);
        }
        return byName(a, b);
    });
}
