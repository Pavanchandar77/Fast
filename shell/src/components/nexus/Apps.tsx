import { useEffect, useRef } from "react";
import { FILES, previewBody, type AppId, type FileEntry } from "@/lib/nexus/engine";
import { ramSummary, useNexus } from "@/lib/nexus/store";

export function AppBody({ app }: { app: AppId }) {
  if (app === "finder") return <Finder />;
  if (app === "governor") return <Governor />;
  if (app === "shield") return <Shield />;
  if (app === "config") return <Config />;
  if (app === "console") return <Console />;
  if (app === "windows") return <WindowsHost />;
  if (app === "notepad") return <Notepad />;
  return <Arena />;
}

function Finder() {
  const selected = useNexus((s) => s.selectedPath);
  const select = useNexus((s) => s.selectFile);
  const openQuick = useNexus((s) => s.toggleQuicklook);
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="hidden w-36 shrink-0 flex-col gap-1 border-r border-border p-3 text-sm text-muted sm:flex">
        <p className="px-2 pb-1 text-xs tracking-wide text-fg">Places</p>
        {["Desktop", "Documents", "Downloads", "nexus-os", "Vault"].map((p) => (
          <div key={p} className="rounded-lg px-2 py-1.5">
            {p}
          </div>
        ))}
      </aside>
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="w-full text-left text-sm">
          <thead className="sticky top-0 bg-surface text-xs text-muted">
            <tr>
              <th className="px-4 py-2 font-medium">Name</th>
              <th className="px-2 py-2 font-medium">Kind</th>
              <th className="hidden px-2 py-2 font-medium sm:table-cell">Size</th>
            </tr>
          </thead>
          <tbody>
            {FILES.map((file) => {
              const on = file.path === selected;
              return (
                <tr
                  key={file.path}
                  onClick={() => select(file.path)}
                  onDoubleClick={openQuick}
                  className={on ? "bg-accent/15 text-fg" : "text-fg/90"}
                >
                  <td className="max-w-40 truncate px-4 py-2.5 font-medium">{file.name}</td>
                  <td className="px-2 py-2.5 text-muted">{file.kind}</td>
                  <td className="hidden px-2 py-2.5 font-mono text-xs text-muted sm:table-cell">{file.size}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Governor() {
  const procs = useNexus((s) => s.procs);
  const standby = useNexus((s) => s.standbyMb);
  const power = useNexus((s) => s.settings.power);
  const game = useNexus((s) => s.gameMode);
  const purge = useNexus((s) => s.purge);
  const toggleGame = useNexus((s) => s.toggleGame);
  const { live } = ramSummary(procs, standby);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <p className="text-xs text-muted">Working set</p>
          <p className="font-mono text-3xl text-fg">{live}<span className="text-base text-muted"> MB</span></p>
        </div>
        <div className="text-right">
          <p className="text-xs text-muted">Standby</p>
          <p className="font-mono text-lg text-fg">{standby} MB</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={purge} className="rounded-full bg-fg px-4 py-2 text-sm font-medium text-bg">
          Purge standby
        </button>
        <button type="button" onClick={toggleGame} className="rounded-full border border-border px-4 py-2 text-sm text-fg">
          {game ? "Leave game mode" : "Game mode"}
        </button>
      </div>
      <p className="text-sm text-muted">
        {power === "MaxCompute"
          ? "Max Compute. Cores stay unparked and App Nap is off."
          : power === "EcoSilent"
            ? "Eco Silent. Buried apps nap sooner."
            : "Balanced. Focus gets High. Everything else drops to Below Normal, then naps."}
        {game ? " Shell blur and border shaders are suspended." : ""}
      </p>
      <ul className="flex flex-col gap-1">
        {procs.map((p) => (
          <li key={p.pid} className="grid grid-cols-[auto_1fr_auto] items-center gap-3 rounded-lg px-2 py-1.5 text-sm">
            <span className="font-mono text-xs text-muted">{p.pid}</span>
            <span className="truncate">
              {p.name}
              <span className="ml-2 text-xs text-muted">{p.state}</span>
            </span>
            <span className="font-mono text-xs text-fg">{p.ram} MB</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

const HOSTS = [
  "v10.events.data.microsoft.com",
  "telemetry.microsoft.com",
  "watson.telemetry.microsoft.com",
  "settings-win.data.microsoft.com",
];

const VAULT = ["~/.ssh", "~/.gnupg", "~/.aws", "~/.config/nexus-os"];

function Shield() {
  const rules = useNexus((s) => s.rules);
  const raise = useNexus((s) => s.raisePrompt);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-4 text-sm">
      <div className="flex items-center justify-between gap-3">
        <p className="text-fg">Telemetry is dropped at the filter engine. Defender is not the scanner.</p>
        <button type="button" onClick={raise} className="shrink-0 rounded-full bg-fg px-4 py-2 font-medium text-bg">
          Probe outbound
        </button>
      </div>
      <div>
        <p className="pb-2 text-xs text-muted">Blocked hosts</p>
        <ul className="flex flex-col gap-1 font-mono text-xs text-fg">
          {HOSTS.map((h) => (
            <li key={h} className="rounded-lg bg-bg/40 px-3 py-2">{h}</li>
          ))}
        </ul>
      </div>
      <div>
        <p className="pb-2 text-xs text-muted">Decisions</p>
        <ul className="flex flex-col gap-1">
          {rules.map((r) => (
            <li key={r.exe} className="flex justify-between rounded-lg px-1 py-1">
              <span>{r.exe}</span>
              <span className="text-muted">{r.verdict}</span>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <p className="pb-2 text-xs text-muted">Vault</p>
        <ul className="font-mono text-xs text-muted">
          {VAULT.map((v) => (
            <li key={v} className="py-1">{v}</li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Config() {
  const text = useNexus((s) => s.configText);
  const error = useNexus((s) => s.configError);
  const setText = useNexus((s) => s.setConfigText);
  const settings = useNexus((s) => s.settings);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
      <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs text-muted">
        <span>{settings.layout}</span>
        <span>k {settings.stiffness}</span>
        <span>c {settings.damping}</span>
        <span>nap {settings.napSecs}s</span>
        <span>{settings.power}</span>
      </div>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        spellCheck={false}
        className="min-h-0 flex-1 resize-none rounded-xl border border-border bg-bg/50 p-3 font-mono text-xs leading-5 text-fg outline-none"
        aria-label="system.toml"
      />
      <p className={error ? "text-sm text-accent" : "text-sm text-muted"}>
        {error ?? "Applied. Saving this file on Windows hot-reloads the same keys."}
      </p>
    </div>
  );
}

function Console() {
  const logs = useNexus((s) => s.logs);
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [logs]);
  return (
    <div className="min-h-0 flex-1 overflow-auto p-4 font-mono text-xs leading-5 text-fg">
      {logs.map((l) => (
        <p key={l.id}>
          <span className="text-muted">{String(l.id).padStart(2, "0")} </span>
          {l.msg}
        </p>
      ))}
      <div ref={end} />
    </div>
  );
}

function WindowsHost() {
  const open = useNexus((s) => s.openApp);
  const panic = useNexus((s) => s.panicNow);
  const rows = [
    ["Kernel", "ntoskrnl.exe stays resident. Nexus does not load a kernel driver."],
    ["Programs", "Win32 .exe files still launch. Notepad below is one of them."],
    ["DirectX 12", "Arena is an exclusive swapchain. The shell gets out of its way."],
    ["Drivers", "GPU, audio, and the lid switch stay on the Windows stacks."],
    ["Anti-cheat", "No kernel patch, so Vanguard, EAC, and BattlEye keep their view."],
    ["Fail-safe", "If the shell dies, explorer.exe is shown again. You are not locked out."],
  ];
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-4">
      <ul className="flex flex-col gap-3">
        {rows.map(([title, body]) => (
          <li key={title}>
            <p className="text-sm font-medium text-fg">{title}</p>
            <p className="text-sm text-muted">{body}</p>
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={() => open("notepad")} className="rounded-full bg-fg px-4 py-2 text-sm font-medium text-bg">
          Open Notepad
        </button>
        <button type="button" onClick={() => open("arena")} className="rounded-full border border-border px-4 py-2 text-sm text-fg">
          Start Arena
        </button>
        <button type="button" onClick={panic} className="rounded-full border border-border px-4 py-2 text-sm text-fg">
          Restore explorer
        </button>
      </div>
    </div>
  );
}

function Notepad() {
  const value = useNexus((s) => s.notepad);
  const set = useNexus((s) => s.setNotepad);
  return (
    <textarea
      value={value}
      onChange={(e) => set(e.target.value)}
      className="min-h-0 flex-1 resize-none bg-transparent p-4 font-mono text-sm leading-6 text-fg outline-none"
      aria-label="Notepad"
    />
  );
}

function Arena() {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let frame = 0;
    let raf = 0;
    const draw = () => {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      frame += 1;
      ctx.fillStyle = "#0c0c0e";
      ctx.fillRect(0, 0, w, h);
      const x = ((frame * 3) % (w + 80)) - 40;
      ctx.fillStyle = "#6cb6ff";
      ctx.globalAlpha = 0.85;
      ctx.fillRect(x, h * 0.42, 36, 4);
      ctx.globalAlpha = 0.2;
      ctx.fillRect(0, h * 0.5, w, 1);
      ctx.globalAlpha = 1;
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, []);
  return (
    <div className="relative min-h-0 flex-1">
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      <p className="pointer-events-none absolute left-4 top-4 text-xs text-muted">
        Exclusive swapchain. Shell shaders are off while this window is focused.
      </p>
    </div>
  );
}

export function QuickLookCard({ file }: { file: FileEntry }) {
  const body = previewBody(file);
  return (
    <div className="flex max-h-[70dvh] w-[min(560px,calc(100vw-32px))] flex-col overflow-hidden rounded-2xl border border-border nx-material">
      <header className="flex items-center justify-between px-4 py-3">
        <div>
          <p className="text-sm font-medium">{file.name}</p>
          <p className="font-mono text-xs text-muted">{file.path}</p>
        </div>
        <p className="text-xs text-muted">{file.kind}</p>
      </header>
      <pre className="overflow-auto px-4 pb-4 font-mono text-xs leading-5 whitespace-pre-wrap text-fg">{body.body}</pre>
    </div>
  );
}
