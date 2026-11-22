// The plugins that show something to people (GET /api/plugins/panels): `settings` ones under Settings →
// Account, `sidebar` ones as a page of their own in the left menu. Asked for after signing in, and again
// when the owner switches a plugin on or off.
import { reactive } from 'vue';
import { get } from './api';

export interface PanelInfo { id: string, name: string, settings?: boolean, sidebar?: boolean }
export const panels = reactive({ list: [] as PanelInfo[], loaded: false });
export async function loadPanels() {
  try { panels.list = (await get<{ panels: PanelInfo[] }>('/api/plugins/panels')).panels; } catch { panels.list = []; }
  panels.loaded = true;
}
