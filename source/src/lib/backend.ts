import { invoke } from '@tauri-apps/api/core';

export type EngineInstall = {
  name: string;
  version: string;
  path: string;
  source: string;
};

export type UnrealProject = {
  name: string;
  path: string;
  engineAssociation: string | null;
  modifiedUnix: number;
  thumbnail?: string | null;
};

export async function scanEngines(): Promise<EngineInstall[]> {
  return invoke<EngineInstall[]>('scan_engines');
}

export async function scanProjects(): Promise<UnrealProject[]> {
  return invoke<UnrealProject[]>('scan_projects');
}

export async function launchProject(path: string): Promise<void> {
  await invoke('launch_project', { path });
}
