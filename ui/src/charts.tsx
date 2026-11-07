// Tiny SVG time-series chart for the analytics screens (doc 15 §5 items 9, 14).
// shortcut: hand-rolled instead of uPlot (doc 15 §3); ≤ 60 points per series. Swap in
// uPlot when charts need thousands of points or zoom.

import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";

export interface Series<P> {
  label: string;
  /** CSS color, e.g. `var(--series-1)`. */
  color: string;
  value: (p: P) => number;
}

const H = 168;
const PAD = { l: 52, r: 12, t: 10, b: 24 };

/** 1, 2, 5 × 10^n at or above `v`. */
function niceMax(v: number) {
  if (v <= 0) return 1;
  const e = 10 ** Math.floor(Math.log10(v));
  return ([1, 2, 5, 10].find((m) => m * e >= v) ?? 10) * e;
}

export function LineChart<P extends { t: number }>(p: {
  title: string;
  points: P[];
  series: Series<P>[];
  fmt: (n: number) => string;
  tfmt: (t: number) => string;
}) {
  let box!: HTMLDivElement;
  const [w, setW] = createSignal(320);
  const [hover, setHover] = createSignal<number | null>(null);
  onMount(() => {
    const ro = new ResizeObserver(([e]) => setW(Math.max(240, e.contentRect.width)));
    ro.observe(box);
    onCleanup(() => ro.disconnect());
  });

  const n = () => p.points.length;
  const max = createMemo(() => niceMax(Math.max(0, ...p.points.flatMap((pt) => p.series.map((s) => s.value(pt))))));
  const x = (i: number) => PAD.l + (n() < 2 ? 0 : (i / (n() - 1)) * (w() - PAD.l - PAD.r));
  const y = (v: number) => PAD.t + (1 - v / max()) * (H - PAD.t - PAD.b);
  const path = (s: Series<P>) => p.points.map((pt, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(s.value(pt)).toFixed(1)}`).join("");
  const ticks = () => [0, max() / 2, max()];
  const xTicks = () => (n() ? [0, Math.floor((n() - 1) / 2), n() - 1] : []);

  const pick = (clientX: number) => {
    const r = box.getBoundingClientRect();
    const f = (clientX - r.left - PAD.l) / (w() - PAD.l - PAD.r);
    setHover(Math.max(0, Math.min(n() - 1, Math.round(f * (n() - 1)))));
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const h = hover() ?? n() - 1;
    setHover(Math.max(0, Math.min(n() - 1, h + (e.key === "ArrowLeft" ? -1 : 1))));
  };
  const hp = () => {
    const h = hover();
    return h === null ? undefined : p.points[h];
  };

  return (
    <figure class="chart">
      <figcaption class="chart-head">
        <h3>{p.title}</h3>
        <Show when={p.series.length > 1}>
          <ul class="legend">
            <For each={p.series}>
              {(s) => (
                <li>
                  <span class="key" style={{ background: s.color }} />
                  {s.label}
                </li>
              )}
            </For>
          </ul>
        </Show>
      </figcaption>
      <div
        class="chart-box"
        ref={box}
        tabindex="0"
        role="img"
        aria-label={`${p.title}. Use arrow keys to read values.`}
        onPointerMove={(e) => pick(e.clientX)}
        onPointerLeave={() => setHover(null)}
        onFocus={() => setHover(n() - 1)}
        onBlur={() => setHover(null)}
        onKeyDown={onKey}
      >
        <svg width={w()} height={H} aria-hidden="true">
          <For each={ticks()}>
            {(v) => (
              <>
                <line class="grid-line" x1={PAD.l} x2={w() - PAD.r} y1={y(v)} y2={y(v)} />
                <text class="tick" x={PAD.l - 8} y={y(v)} dy="0.32em" text-anchor="end">
                  {p.fmt(v)}
                </text>
              </>
            )}
          </For>
          <For each={xTicks()}>
            {(i, k) => (
              <text class="tick" x={x(i)} y={H - 6} text-anchor={k() === 0 ? "start" : k() === 2 ? "end" : "middle"}>
                {p.tfmt(p.points[i].t)}
              </text>
            )}
          </For>
          <For each={p.series}>{(s) => <path d={path(s)} fill="none" stroke={s.color} stroke-width="2" stroke-linejoin="round" />}</For>
          <Show when={hover() !== null && hp()}>
            {(pt) => (
              <>
                <line class="crosshair" x1={x(hover()!)} x2={x(hover()!)} y1={PAD.t} y2={H - PAD.b} />
                <For each={p.series}>
                  {(s) => <circle cx={x(hover()!)} cy={y(s.value(pt()))} r="4" fill={s.color} stroke="var(--surface)" stroke-width="2" />}
                </For>
              </>
            )}
          </Show>
        </svg>
        <Show when={hover() !== null && hp()}>
          {(pt) => (
            <div class="tooltip" style={{ left: `${Math.min(x(hover()!) + 12, w() - 170)}px` }}>
              <div class="tooltip-t">{p.tfmt(pt().t)}</div>
              <For each={p.series}>
                {(s) => (
                  <div class="tooltip-row">
                    <span class="line-key" style={{ background: s.color }} />
                    <strong>{p.fmt(s.value(pt()))}</strong>
                    <span class="muted">{s.label}</span>
                  </div>
                )}
              </For>
            </div>
          )}
        </Show>
      </div>
      <details class="chart-table">
        <summary>Table</summary>
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Time</th>
              <For each={p.series}>{(s) => <th scope="col">{s.label}</th>}</For>
            </tr>
          </thead>
          <tbody>
            <For each={p.points}>
              {(pt) => (
                <tr>
                  <td>{p.tfmt(pt.t)}</td>
                  <For each={p.series}>{(s) => <td>{p.fmt(s.value(pt))}</td>}</For>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </details>
    </figure>
  );
}

// ---------- formatting ----------

export const fmtNum = (n: number) =>
  n >= 1e9 ? `${+(n / 1e9).toFixed(1)}B` : n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e4 ? `${+(n / 1e3).toFixed(1)}k` : `${Math.round(n)}`;

export const fmtBytes = (n: number) => {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${i ? +n.toFixed(1) : n} ${u[i]}`;
};

export const fmtUs = (us: number) =>
  us >= 1e6 ? `${+(us / 1e6).toFixed(2)} s` : us >= 1e3 ? `${+(us / 1e3).toFixed(1)} ms` : `${Math.round(us)} µs`;

export const fmtUptime = (s: number) => {
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m ${s % 60}s`;
};

export const fmtClock = (t: number) => new Date(t * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
export const fmtHour = (t: number) =>
  new Date(t * 1000).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" });
