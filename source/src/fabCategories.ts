// Epic's older category IDs and Fab's readable names may describe the same topic.
const aliases:Record<string,string>={
 'assets/2d':'2D', 'assets/animations':'Animations', 'assets/blueprints':'Blueprints',
 'assets/communitysamples':'Community Samples', 'assets/environments':'Environments',
 'assets/fx':'Visual Effects', 'assets/materials':'Materials', 'assets/megascans':'Megascans',
 'assets/showcasedemos':'Showcase Demos', 'assets/textures':'Textures',
 'characters-creatures':'Characters & Creatures', 'objects-decor':'Tools, Objects & Decor',
 'sound-effects':'Sound Effects', 'weapons-combat':'Weapons & Combat', 'music':'Music',
 'ui':'UI & Interfaces',
};
const labels:Record<string,string>={
 '2D':'2D', 'Animations':'Анимации', 'Animation & Rigging':'Анимация и риггинг',
 'Blueprints':'Блюпринты', 'Community Samples':'Примеры сообщества',
 'Environments':'Окружения', 'Visual Effects':'Визуальные эффекты', 'Materials':'Материалы',
 'Megascans':'Megascans', 'Showcase Demos':'Демонстрационные проекты', 'Textures':'Текстуры',
 'Characters & Creatures':'Персонажи и существа', 'Tools, Objects & Decor':'Предметы и декор',
 'Sound Effects':'Звуковые эффекты', 'Weapons & Combat':'Оружие и бой', 'Music':'Музыка',
 'Buildings & Architecture':'Здания и архитектура', 'UI & Interfaces':'Интерфейсы',
 'UI Kits / HUDs':'Наборы UI и HUD', 'Widgets & Controls':'Виджеты и элементы управления',
 'Modeling':'Моделирование', '3D Models':'3D-модели', 'Engine Tools':'Инструменты движка',
 'Game Mechanics':'Игровые механики', 'Artificial Intelligence':'Искусственный интеллект',
 'Nature & Plants':'Природа и растения', 'Nature & Terrain':'Природа и ландшафт',
 'Vehicles & Transportation':'Транспорт', 'Lighting':'Освещение', 'Shaders':'Шейдеры',
 'Physics':'Физика', 'Network & Multiplayer':'Сеть и мультиплеер',
};
export function categoryKey(value:string):string {
 const trimmed=value.trim();return (aliases[trimmed.toLowerCase()]||trimmed).toLowerCase();
}
export function libraryTopics(items:{categories:string[]}[]):{key:string;label:string}[] {
 const topics=new Map<string,string>();
 for(const item of items)for(const value of item.categories||[]){
  const canonical=aliases[value.trim().toLowerCase()]||value.trim();if(!canonical)continue;
  const key=categoryKey(canonical);topics.set(key,labels[canonical]||canonical);
 }
 return [...topics].map(([key,label])=>({key,label}));
}
