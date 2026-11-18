import { create } from "zustand";
import {
  APP_META,
  DEFAULT_SETTINGS,
  DEFAULT_TOML,
  FILES,
  type AppId,
  type FileEntry,
  type LayoutMode,
  type PowerProfile,
  type Proc,
  type Rect,
  type Settings,
  type Win,
  defaultRect,
  neighborId,
  patchTomlLine,
  readToml,
  workingSet,
} from "./engine";
import { setSpring } from "./motion";

export interface LogLine {
  id: number;
  msg: string;
}

interface NexusStore {
  workspace: number;
  windows: Win[];
  focusId: string | null;
  zTop: number;
  settings: Settings;
  gameMode: boolean;
  gameLocked: boolean;
  volume: number;
  mix: { system: number; arena: number; notepad: number };
  panic: boolean;
  commandOpen: boolean;
  commandNote: string;
  quicklook: FileEntry | null;
  selectedPath: string;
  configText: string;
  configError: string | null;
  notepad: string;
  logs: LogLine[];
  logSeq: number;
  procs: Proc[];
  standbyMb: number;
  prompt: { exe: string; host: string } | null;
  rules: { exe: string; verdict: "Block" | "LAN" | "Allow" }[];
  vp: { w: number; h: number };
  menu: "nexus" | "volume" | null;
  setVp: (vp: { w: number; h: number }) => void;
  log: (msg: string) => void;
  focus: (id: string) => void;
  openApp: (app: AppId) => void;
  close: (id: string) => void;
  minimize: (id: string) => void;
  toggleFullscreen: (id: string) => void;
  setWorkspace: (n: number) => void;
  moveFocus: (dir: "left" | "right" | "up" | "down") => void;
  detach: (id: string, rect: Rect) => void;
  commitRect: (id: string, rect: Rect) => void;
  setLayout: (layout: LayoutMode) => void;
  setPower: (power: PowerProfile) => void;
  toggleGame: () => void;
  purge: () => void;
  setVolume: (n: number) => void;
  setMix: (key: "system" | "arena" | "notepad", n: number) => void;
  setMenu: (menu: "nexus" | "volume" | null) => void;
  setCommandOpen: (open: boolean) => void;
  setCommandNote: (note: string) => void;
  selectFile: (path: string) => void;
  toggleQuicklook: () => void;
  closeQuicklook: () => void;
  setConfigText: (text: string) => void;
  setNotepad: (text: string) => void;
  raisePrompt: () => void;
  resolvePrompt: (verdict: "Block" | "LAN" | "Allow") => void;
  killProc: (name: string) => void;
  panicNow: () => void;
  resume: () => void;
  tick: (now: number) => void;
}

function coreProcs(): Proc[] {
  return [
    {
      pid: 4,
      name: "ntoskrnl.exe",
      app: null,
      base: 18,
      ram: 18,
      priority: "System",
      state: "kernel",
      lastActive: 0,
      nappable: false,
    },
    {
      pid: 812,
      name: "explorer.exe",
      app: null,
      base: 8,
      ram: 8,
      priority: "BelowNormal",
      state: "purged",
      lastActive: 0,
      nappable: false,
    },
    {
      pid: 1100,
      name: "nexus-session",
      app: null,
      base: 14,
      ram: 14,
      priority: "High",
      state: "shell",
      lastActive: 0,
      nappable: false,
    },
  ];
}

function appProc(app: AppId, pid: number): Proc {
  const meta = APP_META[app];
  return {
    pid,
    name: meta.title,
    app,
    base: meta.baseRam,
    ram: meta.baseRam,
    priority: "BelowNormal",
    state: "background",
    lastActive: 0,
    nappable: meta.nappable,
  };
}

let pidSeq = 1400;

function makeWindow(app: AppId, workspace: number, z: number, index: number): Win {
  return {
    id: app,
    app,
    title: APP_META[app].title,
    workspace,
    minimized: false,
    floating: false,
    fullscreen: false,
    z,
    rect: defaultRect(app, index),
  };
}

const finder = makeWindow("finder", 1, 1, 0);
const governor = makeWindow("governor", 1, 2, 1);

export const useNexus = create<NexusStore>((set, get) => ({
  workspace: 1,
  windows: [finder, governor],
  focusId: "governor",
  zTop: 2,
  settings: DEFAULT_SETTINGS,
  gameMode: false,
  gameLocked: false,
  volume: 0.7,
  mix: { system: 0.5, arena: 0.8, notepad: 0.4 },
  panic: false,
  commandOpen: false,
  commandNote: "",
  quicklook: null,
  selectedPath: "D:/work/nexus-os/config/system.toml",
  configText: DEFAULT_TOML,
  configError: null,
  notepad: "Nexus is the shell.\nntoskrnl is still the kernel.\nClose this with Alt Q. Panic is in the Nexus menu.",
  logs: [
    { id: 1, msg: "Hollow shell hid Shell_TrayWnd. explorer.exe stayed up." },
    { id: 2, msg: "WinEvent hook armed. DWM transitions forced off." },
    { id: 3, msg: "Governor online. Foreground boost and working-set trim armed." },
  ],
  logSeq: 3,
  procs: [
    ...coreProcs(),
    appProc("finder", 1402),
    { ...appProc("governor", 1408), priority: "High", state: "foreground" },
  ],
  standbyMb: 486,
  prompt: null,
  rules: [
    { exe: "DiagTrack", verdict: "Block" },
    { exe: "CompatTelRunner.exe", verdict: "Block" },
  ],
  vp: { w: 1280, h: 800 },
  menu: null,

  setVp: (vp) => set({ vp }),

  log: (msg) =>
    set((s) => {
      const id = s.logSeq + 1;
      const logs = [...s.logs, { id, msg }].slice(-80);
      return { logs, logSeq: id };
    }),

  focus: (id) => {
    const s = get();
    const win = s.windows.find((w) => w.id === id);
    if (!win) return;
    const zTop = s.zTop + 1;
    const arenaFocus = win.app === "arena";
    const gameMode = s.gameLocked ? s.gameMode : arenaFocus;
    set({
      focusId: id,
      zTop,
      workspace: win.workspace,
      gameMode,
      windows: s.windows.map((w) =>
        w.id === id ? { ...w, z: zTop, minimized: false } : w,
      ),
      procs: s.procs.map((p) => {
        if (!p.app) return p;
        if (p.app === win.app) {
          return { ...p, priority: "High", state: "foreground", ram: p.base, lastActive: performance.now() };
        }
        if (p.state === "kernel" || p.state === "shell") return p;
        return {
          ...p,
          priority: "BelowNormal",
          state: p.state === "napping" || p.state === "purged" ? p.state : "background",
        };
      }),
    });
    if (arenaFocus && !s.gameMode) get().log("Arena took the swapchain. Shell shaders suspended.");
    if (!arenaFocus && s.gameMode && !s.gameLocked) get().log("Swapchain released. Shell shaders restored.");
  },

  openApp: (app) => {
    const s = get();
    const existing = s.windows.find((w) => w.app === app);
    if (existing) {
      get().focus(existing.id);
      return;
    }
    pidSeq += 1;
    const zTop = s.zTop + 1;
    const win = makeWindow(app, s.workspace, zTop, s.windows.length);
    set({
      windows: [...s.windows, win],
      focusId: win.id,
      zTop,
      procs: [...s.procs, { ...appProc(app, pidSeq), priority: "High", state: "foreground", lastActive: performance.now() }],
    });
    get().log(`Opened ${APP_META[app].title}.`);
    if (app === "arena") {
      set({ gameMode: true });
      get().log("Game mode. Blur off, timer 0.5 ms, cores left unparked.");
    }
  },

  close: (id) => {
    const s = get();
    const win = s.windows.find((w) => w.id === id);
    if (!win) return;
    const windows = s.windows.filter((w) => w.id !== id);
    const next = [...windows].reverse().find((w) => w.workspace === s.workspace && !w.minimized);
    set({
      windows,
      focusId: next?.id ?? null,
      gameMode: s.gameLocked ? s.gameMode : next?.app === "arena",
      procs: s.procs.filter((p) => p.app !== win.app),
    });
    get().log(`Closed ${win.title} in one frame. WM_CLOSE followed.`);
  },

  minimize: (id) => {
    const s = get();
    const win = s.windows.find((w) => w.id === id);
    if (!win) return;
    set({
      windows: s.windows.map((w) => (w.id === id ? { ...w, minimized: true, fullscreen: false } : w)),
      focusId: s.focusId === id ? null : s.focusId,
      procs: s.procs.map((p) =>
        p.app === win.app ? { ...p, state: "purged", ram: Math.max(4, Math.round(p.base * 0.08)), priority: "BelowNormal" } : p,
      ),
    });
    get().log(`Trimmed ${win.title} working set on minimize.`);
  },

  toggleFullscreen: (id) =>
    set((s) => ({
      windows: s.windows.map((w) => (w.id === id ? { ...w, fullscreen: !w.fullscreen, floating: true } : w)),
    })),

  setWorkspace: (n) => {
    const workspace = Math.min(4, Math.max(1, n));
    const onSpace = get().windows.filter((w) => w.workspace === workspace && !w.minimized);
    set({ workspace, menu: null, focusId: onSpace.at(-1)?.id ?? null });
    get().log(`Workspace ${workspace}.`);
  },

  moveFocus: (dir) => {
    const s = get();
    const id = neighborId(s.windows, s.focusId, dir, s.workspace);
    if (id) get().focus(id);
  },

  detach: (id, rect) =>
    set((s) => ({
      windows: s.windows.map((w) => (w.id === id ? { ...w, floating: true, fullscreen: false, rect } : w)),
    })),

  commitRect: (id, rect) =>
    set((s) => ({
      windows: s.windows.map((w) => (w.id === id ? { ...w, rect, floating: true } : w)),
    })),

  setLayout: (layout) => {
    const configText = patchTomlLine(get().configText, "layout", `"${layout}"`);
    set((s) => ({ settings: { ...s.settings, layout }, configText, menu: null }));
    get().log(`Layout ${layout}.`);
  },

  setPower: (power) => {
    const configText = patchTomlLine(get().configText, "default_power_profile", `"${power}"`);
    set((s) => ({ settings: { ...s.settings, power }, configText, menu: null }));
    get().log(`Power profile ${power}.`);
  },

  toggleGame: () => {
    const next = !get().gameMode;
    set({ gameMode: next, gameLocked: next, menu: null });
    get().log(next ? "Game mode locked. Shell effects suspended." : "Game mode released.");
  },

  purge: () => {
    set((s) => ({
      standbyMb: 36,
      procs: s.procs.map((p) =>
        p.state === "napping" || p.state === "background" || p.state === "purged"
          ? { ...p, state: p.app ? "purged" : p.state, ram: p.app ? Math.max(4, Math.round(p.base * 0.08)) : p.ram }
          : p,
      ),
    }));
    get().log("Standby list purged. Foreground working sets kept.");
  },

  setVolume: (volume) => set({ volume }),
  setMix: (key, n) => set((s) => ({ mix: { ...s.mix, [key]: n } })),
  setMenu: (menu) => set({ menu }),
  setCommandOpen: (commandOpen) => set({ commandOpen, menu: null, commandNote: commandOpen ? get().commandNote : "" }),
  setCommandNote: (commandNote) => set({ commandNote }),
  selectFile: (selectedPath) => set({ selectedPath }),
  toggleQuicklook: () => {
    const s = get();
    if (s.quicklook) {
      set({ quicklook: null });
      return;
    }
    const file = FILES.find((f) => f.path === s.selectedPath) ?? FILES[0];
    set({ quicklook: file });
  },
  closeQuicklook: () => set({ quicklook: null }),
  setConfigText: (configText) => {
    const { error, patch } = readToml(configText);
    if (error) {
      set({ configText, configError: error });
      return;
    }
    set((s) => {
      const settings = { ...s.settings, ...patch };
      setSpring(settings.stiffness, settings.damping, settings.mass);
      return { configText, configError: null, settings };
    });
  },
  setNotepad: (notepad) => set({ notepad }),
  raisePrompt: () =>
    set({
      prompt: { exe: "Updater.exe", host: "downloads.vendor.example" },
    }),
  resolvePrompt: (verdict) => {
    const prompt = get().prompt;
    if (!prompt) return;
    set((s) => ({
      prompt: null,
      rules: [...s.rules, { exe: prompt.exe, verdict }],
    }));
    get().log(`${prompt.exe} → ${verdict}.`);
  },
  killProc: (name) => {
    const q = name.toLowerCase();
    const proc = get().procs.find((p) => p.name.toLowerCase().includes(q) && p.app);
    if (!proc?.app) {
      get().log(`No user process matches "${name}". Kernel processes are not killable here.`);
      return;
    }
    const win = get().windows.find((w) => w.app === proc.app);
    if (win) get().close(win.id);
    get().log(`Terminated ${proc.name}.`);
  },
  panicNow: () => {
    set({ panic: true, menu: null, commandOpen: false, quicklook: null });
  },
  resume: () => {
    set({ panic: false });
    get().log("Nexus retook the session. explorer.exe taskbar hidden again.");
  },
  tick: (now) => {
    const power = get().settings.power;
    const napSecs = get().settings.napSecs;
    const napMs =
      power === "MaxCompute" ? Number.POSITIVE_INFINITY : napSecs * 1000 * (power === "EcoSilent" ? 0.45 : 1);
    set((s) => ({
      procs: s.procs.map((p) => {
        if (!p.nappable || p.state === "foreground" || p.state === "purged") return p;
        const focused = s.windows.find((w) => w.id === s.focusId)?.app;
        if (p.app && p.app === focused) return p;
        if (p.lastActive === 0) return { ...p, lastActive: now };
        if (now - p.lastActive > napMs && p.state !== "napping") {
          return { ...p, state: "napping", ram: Math.max(4, Math.round(p.base * 0.12)), priority: "BelowNormal" };
        }
        return p;
      }),
    }));
  },
}));

export function ramSummary(procs: Proc[], standbyMb: number) {
  return { live: workingSet(procs), standby: standbyMb };
}
