// The jump HUD (ticket #136): `2/7 · needs input · SHOP-142`, or the empty state whose `Enter` opens a view.

export const HUD_MS = 2500;

export interface HudEnter {
  action: string;
  label: string;
}

class HudStore {
  text = $state('');
  enter = $state<HudEnter | null>(null);
  #timer: ReturnType<typeof setTimeout> | undefined;

  show(text: string, enter: HudEnter | null = null): void {
    this.text = text;
    this.enter = enter;
    clearTimeout(this.#timer);
    // one-shot: hide the HUD
    this.#timer = setTimeout(() => this.hide(), HUD_MS);
  }

  hide(): void {
    clearTimeout(this.#timer);
    this.text = '';
    this.enter = null;
  }
}

export const hud = new HudStore();
