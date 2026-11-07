#!/usr/bin/env python3
"""Renders a throughput bar chart (SVG, light/dark aware) from load.py results.

Usage: python3 bench/scenarios/chart.py OUT.svg LABEL=results.json [LABEL=results.json ...]
Bars show the median units/s per scenario for each run; doc 17 §4 gates are drawn as ticks.
"""
import json, statistics as st, sys

GATES = {"M1": 60_000, "M2*": 50_000, "M3": 15_000, "M4": 20_000, "M5": 100_000}
COLORS = ["var(--a)", "var(--b)", "var(--c)"]

out, runs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]
data = [(label, json.load(open(path))["results"]) for label, path in runs]
names = [n for n in data[0][1] if "open-loop" not in n]
med = {(l, n): st.median(r["rps"] for r in res[n]["reps"]) for l, res in data for n in names if n in res}
top = max(list(med.values()) + list(GATES.values()))
step = 20_000
top = (int(top) // step + 1) * step

W, left, right, bar, gap, pad = 760, 250, 30, 16, 14, 4
plot = W - left - right
group = len(data) * (bar + pad) + gap
H = 70 + len(names) * group + 40
x = lambda v: left + v / top * plot

s = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" font-family="system-ui,-apple-system,Segoe UI,sans-serif" role="img" aria-label="Monolith throughput by scenario">',
     "<style>:root{--fg:#111827;--mute:#6b7280;--grid:#e5e7eb;--bg:#ffffff;--a:#D90F23;--b:#6b7280;--c:#111827}"
     "@media (prefers-color-scheme:dark){:root{--fg:#e5e7eb;--mute:#9ca3af;--grid:#1f2430;--bg:#0B0D12;--a:#FF4D55;--b:#9ca3af;--c:#e5e7eb}}"
     "text{fill:var(--fg)} .m{fill:var(--mute)}</style>",
     f'<rect width="{W}" height="{H}" rx="8" fill="var(--bg)"/>',
     f'<text x="{left}" y="26" font-size="15" font-weight="600">Throughput per scenario (median, higher is better)</text>']
lx = left
for i, (label, _) in enumerate(data):
    s.append(f'<rect x="{lx}" y="38" width="12" height="12" rx="2" fill="{COLORS[i]}"/><text x="{lx + 18}" y="48" font-size="12">{label}</text>')
    lx += 30 + 7 * len(label)
s.append(f'<line x1="{lx}" y1="44" x2="{lx + 12}" y2="44" stroke="var(--fg)" stroke-width="2" stroke-dasharray="3 2"/><text x="{lx + 18}" y="48" font-size="12" class="m">doc 17 gate</text>')
y0 = 64
for v in range(0, top + 1, step):
    s.append(f'<line x1="{x(v):.1f}" y1="{y0}" x2="{x(v):.1f}" y2="{H - 30}" stroke="var(--grid)"/>')
    s.append(f'<text x="{x(v):.1f}" y="{H - 14}" font-size="11" text-anchor="middle" class="m">{v // 1000}k</text>')
for gi, n in enumerate(names):
    gy = y0 + 6 + gi * group
    sid = n.split()[0]
    unit = " rows/s" if sid == "M5" else "/s"
    s.append(f'<text x="{left - 10}" y="{gy + len(data) * (bar + pad) / 2 + 4:.1f}" font-size="12" text-anchor="end">{n}</text>')
    for i, (label, _) in enumerate(data):
        v = med.get((label, n))
        if v is None:
            continue
        by = gy + i * (bar + pad)
        s.append(f'<rect x="{left}" y="{by}" width="{x(v) - left:.1f}" height="{bar}" rx="2" fill="{COLORS[i]}"><title>{label}: {v:,.0f}{unit}</title></rect>')
        s.append(f'<text x="{x(v) + 5:.1f}" y="{by + bar - 4}" font-size="11" class="m">{v / 1000:.1f}k</text>')
    if sid in GATES:
        gx = x(GATES[sid])
        s.append(f'<line x1="{gx:.1f}" y1="{gy - 3}" x2="{gx:.1f}" y2="{gy + len(data) * (bar + pad)}" stroke="var(--fg)" stroke-width="2" stroke-dasharray="3 2"/>')
s.append("</svg>")
open(out, "w").write("\n".join(s) + "\n")
print(out)
