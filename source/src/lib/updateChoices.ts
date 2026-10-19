import type {VaultPackage} from '../Vault';
import type {FabItem} from '../EpicLibrary';
import {engineVersion,materialVersions} from './materialVersions';

export function updateChoices(local:VaultPackage,item:FabItem) {
 const engines=new Set(local.engines.map(engineVersion));
 const choices=materialVersions(item.versions).filter(v=>engines.has(v.engine));
 const exact=choices.find(v=>v.artifactId===local.artifactId&&v.engine===local.selectedEngine);
 const artifacts=new Set(choices.map(v=>v.artifactId));
 return {choices,defaultKey:exact?.key||(artifacts.size===1?choices[0]?.key:'')||''};
}
