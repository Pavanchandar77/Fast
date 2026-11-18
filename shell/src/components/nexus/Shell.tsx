import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AppWindow,
  Box,
  Folder,
  Gauge,
  Shield,
  SlidersHorizontal,
  SquareTerminal,
  StickyNote,
  type LucideIcon,
} from "lucide-react";
import {
  DOCK,
  FILES,
  APP_META,
  evalMath,
  fuzzyScore,
  placeWindows,
  shellChrome,
  type AppId,
  type Rect,
  type Win,
} from "@/lib/nexus/engine";
import {
  beginDrag,
  bindWindow,
  dragTo,
  dropWindow,
  endDrag,
  readBody,
  resizeTo,
  retarget,
  setSpring,
  startMotion,
} from "@/lib/nexus/motion";
import { ramSummary, useNexus } from "@/lib/nexus/store";
import { AppBody, QuickLookCard } from "./Apps";

const ICONS: Record<AppId, LucideIcon> = {
  finder: Folder,
  governor: Gauge,
  shield: Shield,
  config: SlidersHorizontal,
  console: SquareTerminal,
  windows: AppWindow,
  notepad: StickyNote,
  arena: Box,
};

export function Shell() {
  const panic = useNexus((s) => s.panic);
  const game = useNexus((s) => s.gameMode);
  const blur = useNexus((s) => s.settings.blur);
  useChrome();
  if (panic) return <PanicScreen />;
  return (
    <div
      className="nexus-root relative h-dvh w-full overflow-hidden text-fg"
      data-game={game ? "true" : "false"}
      style={{ ["--nx-blur" as string]: game ? "0px" : `${blur}px` }}
    >
      <div className="nx-desktop absolute inset-0" />
      <WindowLayer />
      <MenuBar />
      <Dock />
      <CommandBar />
      <QuickLook />
      <OutboundToast />
    </div>
  );
}

function useChrome() {
  const setVp = useNexus((s) => s.setVp);
  const tick = useNexus((s) => s.tick);
  useLayoutEffect(() => {
    const read = () => setVp({ w: window.innerWidth, h: window.innerHeight });
    read();
    startMotion();
    window.addEventListener("resize", read);
    return () => window.removeEventListener("resize", read);
  }, [setVp]);
  useEffect(() => {
    const id = window.setInterval(() => tick(performance.now()), 1000);
    return () => window.clearInterval(id);
  }, [tick]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useNexus.getState();
      const tag = (e.target as HTMLElement | null)?.tagName;
      const typing = tag === "INPUT" || tag === "TEXTAREA";
      if (e.key === "Escape") {
        s.setCommandOpen(false);
        s.closeQuicklook();
        s.setMenu(null);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        s.setCommandOpen(!s.commandOpen);
        return;
      }
      if (e.ctrlKey && e.altKey && e.shiftKey && e.key === "Escape") {
        e.preventDefault();
        s.panicNow();
        return;
      }
      if (typing) return;
      if (e.code === "Space") {
        e.preventDefault();
        s.toggleQuicklook();
        return;
      }
      if (!e.altKey) return;
      const key = e.key.toLowerCase();
      if (key >= "1" && key <= "4") {
        e.preventDefault();
        s.setWorkspace(Number(key));
      } else if (key === "q") {
        e.preventDefault();
        if (s.focusId) s.close(s.focusId);
      } else if (key === "h") s.moveFocus("left");
      else if (key === "l") s.moveFocus("right");
      else if (key === "k") s.moveFocus("up");
      else if (key === "j") s.moveFocus("down");
      else if (key === " ") {
        e.preventDefault();
        s.setCommandOpen(true);
      } else if (key === "f" && s.focusId) s.toggleFullscreen(s.focusId);
      else if (key === "v") s.setLayout(s.settings.layout === "floating" ? "dwindle" : "floating");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

function MenuBar() {
  const vp = useNexus((s) => s.vp);
  const workspace = useNexus((s) => s.workspace);
  const setWorkspace = useNexus((s) => s.setWorkspace);
  const windows = useNexus((s) => s.windows);
  const focusId = useNexus((s) => s.focusId);
  const procs = useNexus((s) => s.procs);
  const standby = useNexus((s) => s.standbyMb);
  const power = useNexus((s) => s.settings.power);
  const setPower = useNexus((s) => s.setPower);
  const menu = useNexus((s) => s.menu);
  const setMenu = useNexus((s) => s.setMenu);
  const setLayout = useNexus((s) => s.setLayout);
  const layout = useNexus((s) => s.settings.layout);
  const toggleGame = useNexus((s) => s.toggleGame);
  const purge = useNexus((s) => s.purge);
  const panicNow = useNexus((s) => s.panicNow);
  const game = useNexus((s) => s.gameMode);
  const volume = useNexus((s) => s.volume);
  const setVolume = useNexus((s) => s.setVolume);
  const mix = useNexus((s) => s.mix);
  const setMix = useNexus((s) => s.setMix);
  const setCommandOpen = useNexus((s) => s.setCommandOpen);
  const [clock, setClock] = useState("--:--");
  useEffect(() => {
    const tick = () =>
      setClock(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
    tick();
    const id = window.setInterval(tick, 15000);
    return () => window.clearInterval(id);
  }, []);
  const bar = shellChrome(vp, 10).bar;
  const title = windows.find((w) => w.id === focusId)?.title ?? "Desktop";
  const { live } = ramSummary(procs, standby);
  const cycle = () => {
    const order = ["EcoSilent", "Balanced", "MaxCompute"] as const;
    const i = order.indexOf(power);
    setPower(order[(i + 1) % order.length]);
  };
  const powerLabel = power === "EcoSilent" ? "Eco" : power === "MaxCompute" ? "Max" : "Bal";
  return (
    <header
      className="nx-material fixed inset-x-0 top-0 z-40 flex items-center gap-2 border-b border-white/10 px-2 text-xs"
      style={{ height: bar }}
    >
      <button type="button" className="flex items-center gap-2 rounded-md px-2 text-sm font-medium" onClick={() => setMenu(menu === "nexus" ? null : "nexus")} aria-expanded={menu === "nexus"}>
        <Mark />
        Nexus
      </button>
      {menu === "nexus" && (
        <div className="absolute left-2 top-full mt-1 w-56 rounded-xl border border-border nx-material p-1 text-sm shadow-none">
          <MenuItem label={`Layout · ${layout}`} onClick={() => setLayout(layout === "floating" ? "dwindle" : layout === "dwindle" ? "master_stack" : "floating")} />
          <MenuItem label={game ? "Leave game mode" : "Game mode"} onClick={toggleGame} />
          <MenuItem label="Purge standby RAM" onClick={() => { purge(); setMenu(null); }} />
          <MenuItem label="Restore explorer" onClick={panicNow} />
        </div>
      )}
      <div className="flex items-center gap-0.5">
        {[1, 2, 3, 4].map((n) => (
          <button
            key={n}
            type="button"
            onClick={() => setWorkspace(n)}
            className={
              n === workspace
                ? "grid h-7 w-7 place-items-center rounded-md bg-fg text-bg"
                : "grid h-7 w-7 place-items-center rounded-md text-muted"
            }
            aria-label={`Workspace ${n}`}
          >
            {n}
          </button>
        ))}
      </div>
      <p className="hidden min-w-0 flex-1 truncate text-center text-muted sm:block">{title}</p>
      <div className="ml-auto flex items-center gap-1 sm:gap-2">
        {game && <span className="rounded-md bg-accent px-1.5 py-0.5 font-medium text-bg">Game</span>}
        <span className="hidden font-mono text-muted md:inline">{Math.round(live)} MB</span>
        <button type="button" className="rounded-md px-2 py-1 text-muted" onClick={() => setMenu(menu === "volume" ? null : "volume")} aria-label="Volume">
          {Math.round(volume * 100)}
        </button>
        {menu === "volume" && (
          <div className="absolute right-16 top-full mt-1 w-56 rounded-xl border border-border nx-material p-3 text-sm">
            <Mix label="Output" value={volume} onChange={setVolume} />
            <Mix label="System" value={mix.system} onChange={(n) => setMix("system", n)} />
            <Mix label="Arena" value={mix.arena} onChange={(n) => setMix("arena", n)} />
            <Mix label="Notepad" value={mix.notepad} onChange={(n) => setMix("notepad", n)} />
          </div>
        )}
        <button type="button" onClick={cycle} className="rounded-md px-2 py-1 font-medium text-fg">
          {powerLabel}
        </button>
        <button type="button" onClick={() => setCommandOpen(true)} className="rounded-md px-2 py-1 text-muted">
          Search
        </button>
        <time className="px-1 font-medium tabular-nums" suppressHydrationWarning>
          {clock}
        </time>
      </div>
    </header>
  );
}

function MenuItem({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="block w-full rounded-lg px-3 py-2 text-left hover:bg-fg/10">
      {label}
    </button>
  );
}

function Mix({ label, value, onChange }: { label: string; value: number; onChange: (n: number) => void }) {
  return (
    <label className="mb-2 flex items-center gap-3 last:mb-0">
      <span className="w-16 text-muted">{label}</span>
      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-[var(--color-accent)]"
        aria-label={label}
      />
    </label>
  );
}

function Mark() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <rect x="1" y="3" width="8" height="8" rx="2" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <rect x="5" y="1" width="8" height="8" rx="2" fill="currentColor" />
    </svg>
  );
}

function Dock() {
  const vp = useNexus((s) => s.vp);
  const windows = useNexus((s) => s.windows);
  const openApp = useNexus((s) => s.openApp);
  const focus = useNexus((s) => s.focus);
  const minimize = useNexus((s) => s.minimize);
  const focusId = useNexus((s) => s.focusId);
  const ref = useRef<HTMLDivElement>(null);
  const chrome = shellChrome(vp, 10);
  const magnify = (clientX: number | null) => {
    const root = ref.current;
    if (!root) return;
    for (const el of root.querySelectorAll<HTMLElement>("[data-dock-icon]")) {
      if (clientX === null || chrome.narrow) {
        el.style.transform = "scale(1)";
        continue;
      }
      const r = el.getBoundingClientRect();
      const dist = Math.abs(clientX - (r.left + r.width / 2));
      const t = Math.max(0, 1 - dist / 120);
      el.style.transform = `scale(${1 + t * 0.45})`;
    }
  };
  return (
    <div className="fixed inset-x-0 bottom-0 z-40 flex justify-center px-3 pb-3" style={{ height: chrome.dock }}>
      <div
        ref={ref}
        className="nx-material flex max-w-full items-end gap-1 overflow-x-auto rounded-2xl border border-white/10 px-2 py-2"
        onMouseMove={(e) => magnify(e.clientX)}
        onMouseLeave={() => magnify(null)}
      >
        {DOCK.map((app) => {
          const Icon = ICONS[app];
          const open = windows.filter((w) => w.app === app);
          const running = open.length > 0;
          const focused = open.some((w) => w.id === focusId && !w.minimized);
          return (
            <button
              key={app}
              type="button"
              aria-label={APP_META[app].dock}
              className="flex w-12 shrink-0 flex-col items-center gap-1"
              onClick={() => {
                const win = open[0];
                if (!win) openApp(app);
                else if (focused) minimize(win.id);
                else focus(win.id);
              }}
            >
              <span
                data-dock-icon
                className="grid h-11 w-11 place-items-center rounded-xl bg-fg/10 text-fg"
                style={{ transformOrigin: "bottom center" }}
              >
                <Icon size={18} strokeWidth={1.75} />
              </span>
              <span className={running ? "h-1 w-1 rounded-full bg-fg" : "h-1 w-1 rounded-full bg-transparent"} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

function WindowLayer() {
  const windows = useNexus((s) => s.windows);
  const workspace = useNexus((s) => s.workspace);
  const layout = useNexus((s) => s.settings.layout);
  const vp = useNexus((s) => s.vp);
  const gap = useNexus((s) => s.settings.gap);
  const masterRatio = useNexus((s) => s.settings.masterRatio);
  const focusId = useNexus((s) => s.focusId);
  const stiffness = useNexus((s) => s.settings.stiffness);
  const damping = useNexus((s) => s.settings.damping);
  const mass = useNexus((s) => s.settings.mass);
  const rects = placeWindows(windows, { workspace, layout, vp, gap, masterRatio, focusId });
  useEffect(() => {
    setSpring(stiffness, damping, mass);
  }, [stiffness, damping, mass]);
  const visible = windows
    .filter((w) => w.workspace === workspace && !w.minimized && rects[w.id])
    .sort((a, b) => a.z - b.z);
  return (
    <>
      {visible.map((win) => (
        <WindowFrame key={win.id} win={win} rect={rects[win.id]} active={win.id === focusId} />
      ))}
    </>
  );
}

function WindowFrame({ win, rect, active }: { win: Win; rect: Rect; active: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const focus = useNexus((s) => s.focus);
  const close = useNexus((s) => s.close);
  const minimize = useNexus((s) => s.minimize);
  const zoom = useNexus((s) => s.toggleFullscreen);
  const detach = useNexus((s) => s.detach);
  const commitRect = useNexus((s) => s.commitRect);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    bindWindow(win.id, el, rect);
    return () => dropWindow(win.id);
  }, [win.id]);
  useLayoutEffect(() => {
    retarget(win.id, rect);
  }, [win.id, rect.x, rect.y, rect.w, rect.h]);
  const onDrag = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const body = readBody(win.id);
    if (!body) return;
    focus(win.id);
    detach(win.id, body);
    beginDrag(win.id);
    const sx = e.clientX;
    const sy = e.clientY;
    const ox = body.x;
    const oy = body.y;
    let lx = e.clientX;
    let ly = e.clientY;
    let lt = performance.now();
    const move = (ev: PointerEvent) => {
      const now = performance.now();
      const dt = Math.max(0.008, (now - lt) / 1000);
      dragTo(win.id, ox + ev.clientX - sx, oy + ev.clientY - sy, (ev.clientX - lx) / dt, (ev.clientY - ly) / dt);
      lx = ev.clientX;
      ly = ev.clientY;
      lt = now;
    };
    const up = () => {
      const next = endDrag(win.id);
      if (next) commitRect(win.id, next);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  const onResize = (e: React.PointerEvent) => {
    e.stopPropagation();
    const body = readBody(win.id);
    if (!body) return;
    detach(win.id, body);
    beginDrag(win.id);
    const sx = e.clientX;
    const sy = e.clientY;
    const ow = body.w;
    const oh = body.h;
    const move = (ev: PointerEvent) => resizeTo(win.id, Math.max(280, ow + ev.clientX - sx), Math.max(180, oh + ev.clientY - sy));
    const up = () => {
      const next = endDrag(win.id);
      if (next) commitRect(win.id, next);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return (
    <section
      ref={ref}
      data-active={active ? "true" : "false"}
      className="nx-window absolute left-0 top-0 flex flex-col overflow-hidden"
      style={{ zIndex: win.z, width: rect.w, height: rect.h, transform: `translate3d(${rect.x}px, ${rect.y}px, 0)` }}
      onPointerDown={() => focus(win.id)}
    >
      <header
        className="flex h-9 shrink-0 items-center gap-3 px-3 select-none"
        onPointerDown={onDrag}
        onDoubleClick={() => zoom(win.id)}
      >
        <div className="flex items-center gap-1.5" onPointerDown={(e) => e.stopPropagation()}>
          <Light color="var(--color-close)" label={`Close ${win.title}`} onClick={() => close(win.id)} />
          <Light color="var(--color-min)" label={`Minimize ${win.title}`} onClick={() => minimize(win.id)} />
          <Light color="var(--color-zoom)" label={`Zoom ${win.title}`} onClick={() => zoom(win.id)} />
        </div>
        <p className="min-w-0 flex-1 truncate text-center text-xs text-muted">{win.title}</p>
        <span className="w-12" />
      </header>
      <AppBody app={win.app} />
      <button
        type="button"
        aria-label="Resize"
        className="absolute right-0 bottom-0 h-4 w-4 cursor-nwse-resize"
        onPointerDown={onResize}
      />
    </section>
  );
}

function Light({ color, label, onClick }: { color: string; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className="h-3 w-3 rounded-full"
      style={{ background: color }}
    />
  );
}

function CommandBar() {
  const open = useNexus((s) => s.commandOpen);
  const note = useNexus((s) => s.commandNote);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const items = useMemo(() => buildCommands(query), [query, open]);
  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      input.current?.focus();
    }
  }, [open]);
  if (!open) return null;
  const run = (index: number) => {
    const item = items[index];
    if (!item) return;
    item.run();
    if (item.close) useNexus.getState().setCommandOpen(false);
  };
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center px-4 pt-[12dvh]" onMouseDown={() => useNexus.getState().setCommandOpen(false)}>
      <div className="w-[min(560px,100%)] overflow-hidden rounded-2xl border border-border nx-material" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={input}
          value={query}
          onChange={(e) => { setQuery(e.target.value); setActive(0); }}
          placeholder="Apps, files, kill, math"
          aria-label="Command"
          className="w-full bg-transparent px-4 py-3 text-sm text-fg outline-none placeholder:text-muted"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((n) => Math.min(items.length - 1, n + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((n) => Math.max(0, n - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              run(active);
            }
          }}
        />
        <ul className="max-h-80 overflow-auto pb-2">
          {items.map((item, i) => (
            <li key={item.id}>
              <button
                type="button"
                onMouseEnter={() => setActive(i)}
                onClick={() => run(i)}
                className={i === active ? "flex w-full items-center justify-between bg-fg/10 px-4 py-2 text-left" : "flex w-full items-center justify-between px-4 py-2 text-left"}
              >
                <span className="text-sm">{item.title}</span>
                <span className="text-xs text-muted">{item.detail}</span>
              </button>
            </li>
          ))}
        </ul>
        {note && <p className="border-t border-border px-4 py-2 font-mono text-xs text-accent">{note}</p>}
      </div>
    </div>
  );
}

function buildCommands(query: string) {
  const q = query.trim();
  const s = useNexus.getState();
  const items: { id: string; title: string; detail: string; close: boolean; run: () => void }[] = [];
  const math = evalMath(q);
  if (math !== null) {
    items.push({
      id: "math",
      title: String(math),
      detail: "Calculate",
      close: false,
      run: () => s.setCommandNote(String(math)),
    });
  }
  const actions: [string, string, () => void][] = [
    ["Open Finder", "App", () => s.openApp("finder")],
    ["Open Governor", "RAM", () => s.openApp("governor")],
    ["Open Shield", "Firewall", () => s.openApp("shield")],
    ["Open Windows", "Host", () => s.openApp("windows")],
    ["Open Notepad", "exe", () => s.openApp("notepad")],
    ["Start Arena", "DirectX", () => s.openApp("arena")],
    ["Open Config", "system.toml", () => s.openApp("config")],
    ["Open Console", "Log", () => s.openApp("console")],
    ["Float", "Layout", () => s.setLayout("floating")],
    ["Master stack", "Layout", () => s.setLayout("master_stack")],
    ["Dwindle", "Layout", () => s.setLayout("dwindle")],
    ["Eco Silent", "Power", () => s.setPower("EcoSilent")],
    ["Balanced", "Power", () => s.setPower("Balanced")],
    ["Max Compute", "Power", () => s.setPower("MaxCompute")],
    ["Purge standby", "RAM", () => s.purge()],
    ["Game mode", "GPU", () => s.toggleGame()],
    ["Workspace 1", "Space", () => s.setWorkspace(1)],
    ["Workspace 2", "Space", () => s.setWorkspace(2)],
    ["Workspace 3", "Space", () => s.setWorkspace(3)],
    ["Workspace 4", "Space", () => s.setWorkspace(4)],
    ["Restore explorer", "Fail-safe", () => s.panicNow()],
  ];
  if (q.toLowerCase().startsWith("kill ")) {
    const name = q.slice(5);
    items.push({
      id: "kill",
      title: `Kill ${name}`,
      detail: "Process",
      close: true,
      run: () => s.killProc(name),
    });
  }
  for (const [title, detail, run] of actions) {
    const score = fuzzyScore(q, `${title} ${detail}`);
    if (q && score <= 0) continue;
    items.push({ id: title, title, detail, close: true, run });
  }
  for (const file of FILES) {
    const score = fuzzyScore(q, `${file.name} ${file.path}`);
    if (!q || score <= 0) continue;
    items.push({
      id: file.path,
      title: file.name,
      detail: file.kind,
      close: true,
      run: () => {
        s.openApp("finder");
        s.selectFile(file.path);
        s.toggleQuicklook();
      },
    });
  }
  if (!q) return items.slice(0, 8);
  return items.slice(0, 8);
}

function QuickLook() {
  const file = useNexus((s) => s.quicklook);
  const close = useNexus((s) => s.closeQuicklook);
  if (!file) return null;
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/40 px-4" onMouseDown={close}>
      <div onMouseDown={(e) => e.stopPropagation()}>
        <QuickLookCard file={file} />
      </div>
    </div>
  );
}

function OutboundToast() {
  const prompt = useNexus((s) => s.prompt);
  const resolve = useNexus((s) => s.resolvePrompt);
  if (!prompt) return null;
  return (
    <div className="fixed right-4 bottom-24 z-50 w-[min(360px,calc(100vw-32px))] rounded-2xl border border-border nx-material p-4">
      <p className="text-sm font-medium">{prompt.exe}</p>
      <p className="mt-1 font-mono text-xs text-muted">{prompt.host}</p>
      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" className="rounded-full bg-fg px-3 py-1.5 text-xs font-medium text-bg" onClick={() => resolve("Block")}>Block forever</button>
        <button type="button" className="rounded-full border border-border px-3 py-1.5 text-xs" onClick={() => resolve("LAN")}>LAN only</button>
        <button type="button" className="rounded-full border border-border px-3 py-1.5 text-xs" onClick={() => resolve("Allow")}>Allow</button>
      </div>
    </div>
  );
}

function PanicScreen() {
  const resume = useNexus((s) => s.resume);
  return (
    <main className="grid h-dvh place-items-center bg-bg px-6 text-center text-fg">
      <div className="max-w-md">
        <p className="font-mono text-xs text-muted">explorer.exe</p>
        <h1 className="mt-3 text-2xl font-medium">Desktop restored</h1>
        <p className="mt-3 text-sm leading-6 text-muted">
          The shell let go. The taskbar is back, windows were not killed, and the kernel was never involved.
        </p>
        <button type="button" onClick={resume} className="mt-6 rounded-full bg-fg px-5 py-2.5 text-sm font-medium text-bg">
          Resume Nexus
        </button>
      </div>
    </main>
  );
}
