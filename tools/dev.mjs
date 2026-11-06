// `pnpm dev`: backend (cargo run) + admin UI (vite) together, prefixed output, one Ctrl-C stops both.
import { spawn } from "node:child_process";

const procs = [
  { name: "server", color: 36, cmd: "pnpm", args: ["run", "dev:server"] },
  { name: "ui    ", color: 35, cmd: "pnpm", args: ["run", "dev:ui"] },
];
const UI_URL = process.env.MONOLITH_UI_URL ?? "http://localhost:5173";
let code = 0;
let stopping = false;
const children = [];

const stop = (c = 0) => {
  code ||= c;
  if (stopping) return;
  stopping = true;
  for (const ch of children) ch.kill("SIGTERM");
  setTimeout(() => process.exit(code), 3000).unref();
};

for (const p of procs) {
  const ch = spawn(p.cmd, p.args, { stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, FORCE_COLOR: "1" } });
  children.push(ch);
  const tag = `\x1b[${p.color}m[${p.name}]\x1b[0m `;
  const pipe = (stream, out) => {
    let buf = "";
    stream.on("data", (d) => {
      const lines = (buf + d).split("\n");
      buf = lines.pop();
      for (const l of lines) {
        out.write(tag + l + "\n");
        // The server prints its setup link on its own port; in dev the UI lives on the Vite port.
        const m = l.match(/\/_\/#\/install\/[0-9a-f]+/);
        if (m) out.write(`${tag}\x1b[1msetup link for dev UI: ${UI_URL}${m[0]}\x1b[0m\n`);
      }
    });
  };
  pipe(ch.stdout, process.stdout);
  pipe(ch.stderr, process.stderr);
  ch.on("exit", (c, sig) => {
    if (!stopping) console.error(`${tag}exited (${sig ?? c}); stopping the rest`);
    stop(c ?? 1);
  });
}
process.on("SIGINT", () => stop());
process.on("SIGTERM", () => stop());
