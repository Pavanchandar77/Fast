export type AppId =
  | "finder"
  | "governor"
  | "shield"
  | "config"
  | "console"
  | "windows"
  | "notepad"
  | "arena";

export type LayoutMode = "floating" | "master_stack" | "dwindle";
export type PowerProfile = "EcoSilent" | "Balanced" | "MaxCompute";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Win {
  id: string;
  app: AppId;
  title: string;
  workspace: number;
  minimized: boolean;
  floating: boolean;
  fullscreen: boolean;
  z: number;
  rect: Rect;
}

export interface FileEntry {
  path: string;
  name: string;
  kind: string;
  size: string;
  note: string;
}

export interface Proc {
  pid: number;
  name: string;
  app: AppId | null;
  base: number;
  ram: number;
  priority: "System" | "High" | "BelowNormal";
  state: "kernel" | "shell" | "foreground" | "background" | "napping" | "purged";
  lastActive: number;
  nappable: boolean;
}

export interface Settings {
  layout: LayoutMode;
  gap: number;
  masterRatio: number;
  stiffness: number;
  damping: number;
  mass: number;
  blur: number;
  napSecs: number;
  power: PowerProfile;
}

export const FILES: FileEntry[] = [
  {
    path: "C:/Windows/System32/ntoskrnl.exe",
    name: "ntoskrnl.exe",
    kind: "Kernel",
    size: "11.8 MB",
    note: "Windows NT kernel. Nexus never patches this image.",
  },
  {
    path: "C:/Windows/explorer.exe",
    name: "explorer.exe",
    kind: "Shell",
    size: "6.4 MB",
    note: "Kept alive as the COM broker. The taskbar is hidden, not deleted.",
  },
  {
    path: "C:/Windows/System32/dwm.exe",
    name: "dwm.exe",
    kind: "Desktop",
    size: "1.1 MB",
    note: "Desktop Window Manager stays. Nexus only turns its stock animations off.",
  },
  {
    path: "D:/work/nexus-os/config/system.toml",
    name: "system.toml",
    kind: "Config",
    size: "6 KB",
    note: "Single declarative session file. Saves apply without restarting the shell.",
  },
  {
    path: "D:/work/nexus-os/crates/os-governor/src/governor.rs",
    name: "governor.rs",
    kind: "Rust",
    size: "3 KB",
    note: "Foreground boost, working-set trim, and App Nap.",
  },
  {
    path: "D:/work/nexus-os/crates/os-compositor/src/spring.rs",
    name: "spring.rs",
    kind: "Rust",
    size: "5 KB",
    note: "Interruptible damped oscillator. Velocity survives a retarget.",
  },
  {
    path: "D:/work/Ledgera/src/routes/index.tsx",
    name: "index.tsx",
    kind: "TypeScript",
    size: "18 KB",
    note: "Ordinary Win32-hosted app. It keeps running under the shell.",
  },
  {
    path: "D:/games/Arena/Arena.exe",
    name: "Arena.exe",
    kind: "DirectX 12",
    size: "86 MB",
    note: "Exclusive swapchain. Game mode parks the shell shaders and leaves the title alone.",
  },
  {
    path: "C:/Users/me/.ssh/id_ed25519",
    name: "id_ed25519",
    kind: "Vault",
    size: "411 B",
    note: "Vault path. Unlisted executables cannot read it.",
  },
  {
    path: "C:/Users/me/Documents/Notes/session.txt",
    name: "session.txt",
    kind: "Text",
    size: "1 KB",
    note: "Plain text. Space still previews it.",
  },
];

export const APP_META: Record<
  AppId,
  { title: string; dock: string; baseRam: number; nappable: boolean }
> = {
  finder: { title: "Finder", dock: "Finder", baseRam: 42, nappable: true },
  governor: { title: "Governor", dock: "Governor", baseRam: 28, nappable: true },
  shield: { title: "Shield", dock: "Shield", baseRam: 24, nappable: true },
  config: { title: "Config", dock: "Config", baseRam: 18, nappable: true },
  console: { title: "Console", dock: "Console", baseRam: 16, nappable: true },
  windows: { title: "Windows", dock: "Windows", baseRam: 20, nappable: true },
  notepad: { title: "Notepad", dock: "Notepad", baseRam: 12, nappable: true },
  arena: { title: "Arena", dock: "Arena", baseRam: 640, nappable: false },
};

export const DOCK: AppId[] = [
  "finder",
  "governor",
  "shield",
  "windows",
  "notepad",
  "arena",
  "config",
  "console",
];

export const DEFAULT_SETTINGS: Settings = {
  layout: "floating",
  gap: 10,
  masterRatio: 0.55,
  stiffness: 280,
  damping: 26,
  mass: 1,
  blur: 22,
  napSecs: 8,
  power: "Balanced",
};

export const DEFAULT_TOML = `# Nexus session. The kernel is not in this file.
[compositor]
layout = "floating"
inner_gap = 10
master_ratio = 0.55
force_disable_dwm_transitions = true
suppress_native_taskbar = true

[compositor.spring]
stiffness = 280
damping = 26
mass = 1

[compositor.decorations]
blur_radius = 22

[governor]
app_nap_idle_secs = 8
ram_purge_delay_secs = 6
foreground_priority = "High"
background_priority = "BelowNormal"

[hal]
default_power_profile = "Balanced"
lock_bluetooth_a2dp = true
modern_standby_killer = true

[shield]
block_windows_telemetry = true
`;

export function shellChrome(vp: { w: number; h: number }, gap: number) {
  const narrow = vp.w < 760;
  const bar = narrow ? 48 : 28;
  const dock = narrow ? 74 : 86;
  const g = narrow ? 8 : gap;
  return {
    narrow,
    bar,
    dock,
    x: g,
    y: bar + g,
    w: Math.max(160, vp.w - g * 2),
    h: Math.max(160, vp.h - bar - dock - g * 2),
    gap: g,
  };
}

function clamp(rect: Rect, area: { x: number; y: number; w: number; h: number }): Rect {
  const w = Math.min(Math.max(280, rect.w), area.w);
  const h = Math.min(Math.max(180, rect.h), area.h);
  const x = Math.min(Math.max(area.x, rect.x), area.x + area.w - w);
  const y = Math.min(Math.max(area.y, rect.y), area.y + area.h - h);
  return { x, y, w, h };
}

function dwindle(n: number, area: { x: number; y: number; w: number; h: number }, gap: number): Rect[] {
  if (n <= 0) return [];
  if (n === 1) return [{ ...area }];
  const rects: Rect[] = [];
  let x = area.x;
  let y = area.y;
  let w = area.w;
  let h = area.h;
  let vertical = true;
  for (let i = 0; i < n - 1; i += 1) {
    if (vertical) {
      const hw = Math.max(120, (w - gap) / 2);
      rects.push({ x, y, w: hw, h });
      x += hw + gap;
      w = Math.max(120, w - hw - gap);
    } else {
      const hh = Math.max(100, (h - gap) / 2);
      rects.push({ x, y, w, h: hh });
      y += hh + gap;
      h = Math.max(100, h - hh - gap);
    }
    vertical = !vertical;
  }
  rects.push({ x, y, w, h });
  return rects;
}

function master(
  n: number,
  area: { x: number; y: number; w: number; h: number },
  gap: number,
  ratio: number,
): Rect[] {
  if (n <= 0) return [];
  if (n === 1) return [{ ...area }];
  const masterW = Math.max(180, area.w * ratio - gap / 2);
  const restW = Math.max(140, area.w - masterW - gap);
  const rest = n - 1;
  const rh = Math.max(80, (area.h - gap * (rest - 1)) / rest);
  const rects: Rect[] = [{ x: area.x, y: area.y, w: masterW, h: area.h }];
  for (let i = 0; i < rest; i += 1) {
    rects.push({
      x: area.x + masterW + gap,
      y: area.y + i * (rh + gap),
      w: restW,
      h: rh,
    });
  }
  return rects;
}

export function placeWindows(
  windows: Win[],
  opts: {
    workspace: number;
    layout: LayoutMode;
    vp: { w: number; h: number };
    gap: number;
    masterRatio: number;
    focusId: string | null;
  },
): Record<string, Rect> {
  const area = shellChrome(opts.vp, opts.gap);
  const visible = windows.filter((w) => w.workspace === opts.workspace && !w.minimized);
  const out: Record<string, Rect> = {};
  if (area.narrow) {
    const focus = visible.find((w) => w.id === opts.focusId) ?? visible[visible.length - 1];
    if (focus) out[focus.id] = { x: area.x, y: area.y, w: area.w, h: area.h };
    return out;
  }
  if (opts.layout === "floating") {
    for (const w of visible) {
      out[w.id] = w.fullscreen
        ? { x: area.x, y: area.y, w: area.w, h: area.h }
        : clamp(w.rect, area);
    }
    return out;
  }
  const tiled = visible.filter((w) => !w.floating && !w.fullscreen);
  const cells =
    opts.layout === "master_stack"
      ? master(tiled.length, area, area.gap, opts.masterRatio)
      : dwindle(tiled.length, area, area.gap);
  tiled.forEach((w, i) => {
    out[w.id] = cells[i] ?? clamp(w.rect, area);
  });
  for (const w of visible) {
    if (w.fullscreen) out[w.id] = { x: area.x, y: area.y, w: area.w, h: area.h };
    else if (w.floating) out[w.id] = clamp(w.rect, area);
  }
  return out;
}

export function defaultRect(app: AppId, index: number): Rect {
  const col = index % 2;
  const row = Math.floor(index / 2);
  const table: Partial<Record<AppId, Rect>> = {
    finder: { x: 48, y: 48, w: 620, h: 460 },
    governor: { x: 700, y: 72, w: 460, h: 500 },
    windows: { x: 120, y: 88, w: 520, h: 440 },
  };
  return (
    table[app] ?? {
      x: 96 + col * 36,
      y: 64 + row * 32,
      w: 520,
      h: 420,
    }
  );
}

function unquote(raw: string): string {
  const v = raw.trim().replace(/,.*/, "").trim();
  if (v.startsWith('"') && v.endsWith('"')) return v.slice(1, -1);
  return v;
}

export function readToml(text: string): { error: string | null; patch: Partial<Settings> } {
  const patch: Partial<Settings> = {};
  let section = "";
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const raw = lines[i].trim();
    if (!raw || raw.startsWith("#")) continue;
    const sec = raw.match(/^\[([^\]]+)\]$/);
    if (sec) {
      section = sec[1];
      continue;
    }
    if (raw.startsWith("[")) continue;
    const kv = raw.match(/^([A-Za-z0-9_]+)\s*=\s*(.+)$/);
    if (!kv) return { error: `Line ${i + 1} is not key = value.`, patch: {} };
    const key = `${section}.${kv[1]}`;
    const value = unquote(kv[2]);
    if (key === "compositor.layout") {
      if (value === "floating" || value === "master_stack" || value === "dwindle") patch.layout = value;
      else if (value === "scrolling") patch.layout = "dwindle";
      else return { error: `Unknown layout "${value}".`, patch: {} };
    } else if (key === "compositor.inner_gap") {
      const n = Number(value);
      if (!Number.isFinite(n)) return { error: "inner_gap must be a number.", patch: {} };
      patch.gap = n;
    } else if (key === "compositor.master_ratio") {
      const n = Number(value);
      if (!Number.isFinite(n)) return { error: "master_ratio must be a number.", patch: {} };
      patch.masterRatio = n;
    } else if (key === "compositor.spring.stiffness") patch.stiffness = Number(value);
    else if (key === "compositor.spring.damping") patch.damping = Number(value);
    else if (key === "compositor.spring.mass") patch.mass = Number(value);
    else if (key === "compositor.decorations.blur_radius") patch.blur = Number(value);
    else if (key === "governor.app_nap_idle_secs") patch.napSecs = Number(value);
    else if (key === "hal.default_power_profile") {
      if (value === "EcoSilent" || value === "Balanced" || value === "MaxCompute") patch.power = value;
      else return { error: `Unknown power profile "${value}".`, patch: {} };
    }
  }
  return { error: null, patch };
}

export function patchTomlLine(text: string, key: string, formatted: string): string {
  const re = new RegExp(`^(${key}\\s*=\\s*).*$`, "m");
  if (re.test(text)) return text.replace(re, `$1${formatted}`);
  return text;
}

export function fuzzyScore(query: string, text: string): number {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 1;
  let qi = 0;
  let score = 0;
  let prev = -2;
  for (let i = 0; i < t.length && qi < q.length; i += 1) {
    if (t[i] !== q[qi]) continue;
    score += prev === i - 1 ? 4 : 1;
    if (i === 0 || t[i - 1] === "/" || t[i - 1] === " " || t[i - 1] === ".") score += 3;
    prev = i;
    qi += 1;
  }
  return qi === q.length ? score : 0;
}

export function evalMath(input: string): number | null {
  const src = input.trim();
  if (!/^[\d\s.+\-*/()]+$/.test(src) || !/\d/.test(src)) return null;
  try {
    const value = Function(`"use strict"; return (${src})`)() as unknown;
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  } catch {
    return null;
  }
}

export function workingSet(procs: Proc[]): number {
  return procs.reduce((sum, p) => sum + p.ram, 0);
}

export function neighborId(windows: Win[], focusId: string | null, dir: "left" | "right" | "up" | "down", workspace: number): string | null {
  const visible = windows.filter((w) => w.workspace === workspace && !w.minimized);
  const cur = visible.find((w) => w.id === focusId) ?? visible[0];
  if (!cur) return null;
  const cx = cur.rect.x + cur.rect.w / 2;
  const cy = cur.rect.y + cur.rect.h / 2;
  let best: { id: string; d: number } | null = null;
  for (const w of visible) {
    if (w.id === cur.id) continue;
    const x = w.rect.x + w.rect.w / 2;
    const y = w.rect.y + w.rect.h / 2;
    const dx = x - cx;
    const dy = y - cy;
    const ok =
      (dir === "left" && dx < -20) ||
      (dir === "right" && dx > 20) ||
      (dir === "up" && dy < -20) ||
      (dir === "down" && dy > 20);
    if (!ok) continue;
    const d = dx * dx + dy * dy;
    if (!best || d < best.d) best = { id: w.id, d };
  }
  return best?.id ?? cur.id;
}

export function previewBody(file: FileEntry): { heading: string; body: string } {
  if (file.kind === "Rust") {
    return {
      heading: file.name,
      body: "pub fn step(&mut self, dt: f32) {\n    let force = -k * (current - target) - c * velocity;\n    velocity += force / mass * dt;\n    current += velocity * dt;\n}",
    };
  }
  if (file.kind === "Config") {
    return { heading: file.name, body: DEFAULT_TOML };
  }
  if (file.kind === "DirectX 12") {
    return {
      heading: file.name,
      body: "DXGI swapchain, flip model, tearing off.\nNexus detects the exclusive fullscreen and suspends shell blur, dock magnification, and border shaders.\nThe process priority stays with the game.",
    };
  }
  return { heading: file.name, body: file.note };
}
