import type { Rect } from "./engine";

interface Body {
  el: HTMLElement;
  x: number;
  y: number;
  w: number;
  h: number;
  vx: number;
  vy: number;
  vw: number;
  vh: number;
  tx: number;
  ty: number;
  tw: number;
  th: number;
  k: number;
  c: number;
  m: number;
  dragging: boolean;
  hidden: boolean;
}

const bodies = new Map<string, Body>();
let raf = 0;
let last = 0;
let reduced = false;
let spring = { k: 280, c: 26, m: 1 };

function step(pos: number, vel: number, target: number, dt: number): [number, number] {
  const acc = (-spring.k * (pos - target) - spring.c * vel) / spring.m;
  let nextVel = vel + acc * dt;
  let next = pos + nextVel * dt;
  if (Math.abs(next - target) < 0.5 && Math.abs(nextVel) < 0.05) {
    next = target;
    nextVel = 0;
  }
  return [next, nextVel];
}

function paint(b: Body) {
  const el = b.el;
  if (!el.isConnected) return;
  el.style.transform = `translate3d(${b.x}px, ${b.y}px, 0)`;
  el.style.width = `${Math.max(160, b.w)}px`;
  el.style.height = `${Math.max(120, b.h)}px`;
  el.dataset.placed = "true";
}

function frame(now: number) {
  const dt = Math.min(0.032, (now - last) / 1000 || 0.016);
  last = now;
  for (const b of bodies.values()) {
    if (!b.dragging && !reduced) {
      [b.x, b.vx] = step(b.x, b.vx, b.tx, dt);
      [b.y, b.vy] = step(b.y, b.vy, b.ty, dt);
      [b.w, b.vw] = step(b.w, b.vw, b.tw, dt);
      [b.h, b.vh] = step(b.h, b.vh, b.th, dt);
    } else if (reduced) {
      b.x = b.tx;
      b.y = b.ty;
      b.w = b.tw;
      b.h = b.th;
      b.vx = b.vy = b.vw = b.vh = 0;
    }
    paint(b);
  }
  raf = requestAnimationFrame(frame);
}

export function startMotion() {
  if (raf) return;
  reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  last = performance.now();
  raf = requestAnimationFrame(frame);
}

export function setSpring(k: number, c: number, m: number) {
  spring = {
    k: Number.isFinite(k) && k > 0 ? k : 280,
    c: Number.isFinite(c) && c > 0 ? c : 26,
    m: Number.isFinite(m) && m > 0 ? m : 1,
  };
}

export function bindWindow(id: string, el: HTMLElement, rect: Rect) {
  const prev = bodies.get(id);
  if (prev) {
    prev.el = el;
    paint(prev);
    return;
  }
  const b: Body = {
    el,
    x: rect.x,
    y: rect.y,
    w: rect.w,
    h: rect.h,
    vx: 0,
    vy: 0,
    vw: 0,
    vh: 0,
    tx: rect.x,
    ty: rect.y,
    tw: rect.w,
    th: rect.h,
    k: spring.k,
    c: spring.c,
    m: spring.m,
    dragging: false,
    hidden: false,
  };
  bodies.set(id, b);
  paint(b);
}

export function retarget(id: string, rect: Rect) {
  const b = bodies.get(id);
  if (!b || b.dragging) return;
  b.tx = rect.x;
  b.ty = rect.y;
  b.tw = rect.w;
  b.th = rect.h;
}

export function readBody(id: string): Rect | null {
  const b = bodies.get(id);
  if (!b) return null;
  return { x: b.x, y: b.y, w: b.w, h: b.h };
}

export function beginDrag(id: string) {
  const b = bodies.get(id);
  if (!b) return;
  b.dragging = true;
  b.vx = b.vy = 0;
}

export function dragTo(id: string, x: number, y: number, vx: number, vy: number) {
  const b = bodies.get(id);
  if (!b) return;
  b.x = x;
  b.y = y;
  b.tx = x;
  b.ty = y;
  b.vx = vx;
  b.vy = vy;
  paint(b);
}

export function resizeTo(id: string, w: number, h: number) {
  const b = bodies.get(id);
  if (!b) return;
  b.w = w;
  b.h = h;
  b.tw = w;
  b.th = h;
  paint(b);
}

export function endDrag(id: string): Rect | null {
  const b = bodies.get(id);
  if (!b) return null;
  b.dragging = false;
  b.tx = b.x;
  b.ty = b.y;
  b.tw = b.w;
  b.th = b.h;
  return { x: b.x, y: b.y, w: b.w, h: b.h };
}

export function dropWindow(id: string) {
  bodies.delete(id);
}
