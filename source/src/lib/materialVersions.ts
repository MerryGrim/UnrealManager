import type { FabItem } from '../EpicLibrary';

export const engineVersion = (value: string) => value.replace(/^UE_/, '');

export function materialVersions(versions: FabItem['versions']) {
  const choices = new Map<string, { key: string; artifactId: string; engine: string; shared: boolean }>();
  for (const version of versions) {
    if (version.platforms.length && !version.platforms.some(p => p.toLowerCase().includes('win'))) continue;
    for (const raw of version.engines) {
      const engine = engineVersion(raw);
      if (!/^\d+\.\d+(?:\.\d+)?$/.test(engine)) continue;
      const key = JSON.stringify([version.artifactId, engine]);
      choices.set(key, { key, artifactId: version.artifactId, engine, shared: version.engines.length > 1 });
    }
  }
  return [...choices.values()].sort((a,b) => b.engine.localeCompare(a.engine, undefined, { numeric: true }) || a.artifactId.localeCompare(b.artifactId));
}
