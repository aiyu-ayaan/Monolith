#!/usr/bin/env python3
"""HTTP load run against a release `monolith` binary using `oha` (doc 17 §4: M1-M5).

Server is pinned to cores SERVER_CPUS, the load generator to CLIENT_CPUS (doc 17 §1).
Writes bench/results/<stamp>/{results.json,report.md,server.log,oha/*.json}.
Usage: python3 bench/scenarios/load.py [--warm 5] [--secs 20] [--reps 3]
"""
import argparse, json, os, platform, re, shutil, signal, statistics as st
import subprocess, sys, tempfile, time, urllib.request, datetime

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN = os.path.join(ROOT, "target", "release", "monolith")
PORT = 18090
BASE = f"http://127.0.0.1:{PORT}"

ap = argparse.ArgumentParser()
ap.add_argument("--warm", type=int, default=5)
ap.add_argument("--secs", type=int, default=20)
ap.add_argument("--reps", type=int, default=3)
ap.add_argument("--seed", type=int, default=100_000)
ap.add_argument("--server-cpus", default="0-3", help="taskset list for the server (e.g. 0 for a 1-core host)")
ap.add_argument("--client-cpus", default="4-7")
ap.add_argument("--bin", default=BIN, help="server binary to test")
ap.add_argument("--only", default="", help="comma-separated scenario id prefixes, e.g. M1,M4")
a = ap.parse_args()
SERVER_CPUS, CLIENT_CPUS = a.server_cpus, a.client_cpus

stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
out = os.path.join(ROOT, "bench", "results", stamp)
os.makedirs(os.path.join(out, "oha"))
data = tempfile.mkdtemp(prefix="monolith-load-")
logf = open(os.path.join(out, "server.log"), "w")
LOG = open(os.path.join(out, "run.log"), "w")

def log(*m):
    s = time.strftime("%H:%M:%S ") + " ".join(map(str, m))
    print(s, flush=True); LOG.write(s + "\n"); LOG.flush()

def http(method, path, body=None, tok=None):
    r = urllib.request.Request(BASE + path, method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={"content-type": "application/json", **({"authorization": "Bearer " + tok} if tok else {})})
    with urllib.request.urlopen(r) as f:
        raw = f.read()
        return json.loads(raw) if raw else None

def proc_status(pid):
    d = {}
    for l in open(f"/proc/{pid}/status"):
        k, _, v = l.partition(":")
        if k in ("VmRSS", "RssAnon", "RssFile", "VmHWM", "Threads"): d[k] = int(v.split()[0])
    return d

def rss_kb(pid):
    return proc_status(pid)["VmRSS"]

def mem(pid):
    d = proc_status(pid)
    return dict(rss_mb=d["VmRSS"]/1024, anon_mb=d["RssAnon"]/1024, file_mb=d["RssFile"]/1024, threads=d["Threads"])

def cpu_ticks(pid):
    f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return int(f[11]) + int(f[12])  # utime+stime

env = {**os.environ, "MONOLITH_DATA": data, "MONOLITH_ADDR": f"127.0.0.1:{PORT}", "MONOLITH_LOG": "warn"}
srv = subprocess.Popen(["taskset", "-c", SERVER_CPUS, a.bin], env=env, stdout=subprocess.PIPE,
                       stderr=logf, text=True)
pid = srv.pid
def stop():
    srv.send_signal(signal.SIGTERM)
    try: srv.wait(10)
    except Exception: srv.kill()
    shutil.rmtree(data, ignore_errors=True)

try:
    tok = None
    for line in srv.stdout:
        logf.write(line); logf.flush()
        m = re.search(r"/install/([0-9a-f]+)", line)
        if m: tok = m.group(1); break
    for _ in range(100):
        try: http("GET", "/api/v1/system/health"); break
        except Exception: time.sleep(0.1)
    log("server up pid", pid, "idle", mem(pid))
    su = http("POST", "/api/v1/system/install", {"token": tok, "email": "bench@example.com", "password": "bench-pass-1234567"})["token"]
    http("POST", "/api/v1/system/apps", {"name": "bench"}, su)
    pub = {"list": "", "view": "", "create": ""}
    http("POST", "/api/v1/apps/bench/collections", {"name": "items", "fields": [
        {"name": "title", "type": "text", "required": True},
        {"name": "status", "type": "text"}, {"name": "n", "type": "number", "onlyInt": True}], "rules": pub}, su)
    http("POST", "/api/v1/apps/bench/collections", {"name": "gated", "fields": [
        {"name": "title", "type": "text"}, {"name": "status", "type": "text"}],
        "rules": {"view": "status = 'open'", "list": "status = 'open'"}}, su)
    http("POST", "/api/v1/apps/bench/collections", {"name": "sink", "fields": [
        {"name": "title", "type": "text"}, {"name": "n", "type": "number", "onlyInt": True}], "rules": pub}, su)

    # seed in batches of 500 via the batch endpoint (timed: gives a bulk-load number)
    t0 = time.time(); ids = []
    for i in range(0, a.seed, 500):
        ops = [{"op": "create", "collection": "items", "body": {"title": f"item {j}", "status": "open", "n": j}} for j in range(i, i + 500)]
        r = http("POST", "/api/v1/apps/bench/batch", {"ops": ops}, su)
        if len(ids) < 1000: ids += [x.get("id") for x in r.get("results", r.get("items", []))][:1000]
    seed_s = time.time() - t0
    log(f"seeded {a.seed} rows in {seed_s:.1f}s = {a.seed/seed_s:.0f} rows/s (batch 500, authed superuser)")
    ids = [i for i in ids if i]
    log("after seed", mem(pid))
    gid = http("POST", "/api/v1/apps/bench/collections/gated/records", {"title": "g", "status": "open"}, su)["id"]
    vid = ids[len(ids)//2]
    http("GET", f"/api/v1/apps/bench/collections/items/records/{vid}")  # sanity (anon)
    R = "/api/v1/apps/bench"
    batch_body = json.dumps({"ops": [{"op": "create", "collection": "sink", "body": {"title": f"b{j}", "n": j}} for j in range(100)]})
    bf = os.path.join(out, "batch100.json"); open(bf, "w").write(batch_body)
    create_body = json.dumps({"title": "created", "n": 1})
    auth = ["-H", f"authorization: Bearer {su}"]

    # name, path, extra oha args, connections, units-per-request
    scen = [
        ("H0 health (floor)",          "/api/v1/system/health", [], 64, 1),
        ("M1 view by id, public",      f"{R}/collections/items/records/{vid}", [], 64, 1),
        ("M2* view, row rule status='open'", f"{R}/collections/gated/records/{gid}", [], 64, 1),
        ("M3 list keyset limit=50",    f"{R}/collections/items/records?limit=50", [], 64, 1),
        ("M3b list limit=50 filter n>50000", f"{R}/collections/items/records?limit=50&filter=n%3E50000", [], 64, 1),
        ("M4 create single (c=64)",    f"{R}/collections/sink/records", ["-m", "POST", "-d", create_body, "-T", "application/json"], 64, 1),
        ("M4b create single (c=256)",  f"{R}/collections/sink/records", ["-m", "POST", "-d", create_body, "-T", "application/json"], 256, 1),
        ("M5 batch 100 creates (c=16)", f"{R}/batch", ["-m", "POST", "-D", bf, "-T", "application/json"] + auth, 16, 100),
    ]
    # superuser token so batch/create pass any rule; create on `sink` is public anyway.
    results = {}
    def oha(path, extra, c, secs, tag):
        f = os.path.join(out, "oha", tag + ".json")
        cmd = ["taskset", "-c", CLIENT_CPUS, "oha", "--no-tui", "-z", f"{secs}s", "-c", str(c),
               "--output-format", "json"] + extra + [BASE + path]
        # oha: -j = --json in old versions; fall back below if unsupported
        p = subprocess.run(cmd, capture_output=True, text=True)
        if p.returncode != 0:
            raise SystemExit(f"oha failed: {' '.join(cmd)}\n{p.stderr}")
        open(f, "w").write(p.stdout)
        return json.loads(p.stdout)

    if a.only:
        scen = [x for x in scen if x[0].split()[0] in a.only.split(",")]
    for name, path, extra, c, units in scen:
        tag = re.sub(r"\W+", "_", name).strip("_")
        log(f"== {name}  c={c}")
        oha(path, extra, c, a.warm, tag + "_warm")
        reps = []
        for r in range(a.reps):
            t = cpu_ticks(pid); w = time.time()
            j = oha(path, extra, c, a.secs, f"{tag}_r{r+1}")
            cpu = (cpu_ticks(pid) - t) / os.sysconf("SC_CLK_TCK") / (time.time() - w) * 100
            s = j["summary"]; p = j["latencyPercentiles"]
            codes = j.get("statusCodeDistribution", {})
            ok = sum(v for k, v in codes.items() if k.startswith("2"))
            rec = dict(rps=s["requestsPerSec"] * units, reqps=s["requestsPerSec"], success=s["successRate"],
                       p50=p["p50"]*1e3, p90=p["p90"]*1e3, p99=p["p99"]*1e3, p999=p.get("p99.9", float("nan"))*1e3,
                       max=s["slowest"]*1e3,
                       cpu=cpu, rss_mb=rss_kb(pid)/1024, codes=codes, ok=ok)
            reps.append(rec)
            rec.update(mem(pid))
            log(f"   rep{r+1}: {rec['rps']:.0f} units/s  p50 {rec['p50']:.2f} p99 {rec['p99']:.2f} max {rec['max']:.1f} ms  cpu {cpu:.0f}%  rss {rec['rss_mb']:.0f}MB (anon {rec['anon_mb']:.0f}, file {rec['file_mb']:.0f}) thr {rec['threads']}  codes {codes}")
        results[name] = dict(conns=c, units=units, reps=reps)

    # open-loop latency: 50% of M1 median throughput, constant arrival rate
    m1 = results.get("M1 view by id, public", {}).get("reps", [])
    if m1:
        q = int(st.median(r["reqps"] for r in m1) * 0.5)
        log(f"== M1 open-loop @ {q} qps (50% of max)")
        tag = "M1_openloop"; oha(f"{R}/collections/items/records/{vid}", ["-q", str(q)], 64, a.warm, tag + "_warm")
        reps = []
        for r in range(a.reps):
            j = oha(f"{R}/collections/items/records/{vid}", ["-q", str(q)], 64, a.secs, f"{tag}_r{r+1}")
            p = j["latencyPercentiles"]
            reps.append(dict(rps=j["summary"]["requestsPerSec"], reqps=j["summary"]["requestsPerSec"], success=j["summary"]["successRate"],
                             p50=p["p50"]*1e3, p90=p["p90"]*1e3, p99=p["p99"]*1e3, p999=p.get("p99.9", float("nan"))*1e3,
                             max=j["summary"]["slowest"]*1e3, cpu=0, rss_mb=rss_kb(pid)/1024, codes=j.get("statusCodeDistribution", {})))
            log(f"   rep{r+1}: {reps[-1]['rps']:.0f} rps p50 {reps[-1]['p50']:.2f} p99 {reps[-1]['p99']:.2f} p99.9 {reps[-1]['p999']:.2f} max {reps[-1]['max']:.1f} ms")
        results[f"M1 open-loop @{q}qps"] = dict(conns=64, units=1, reps=reps)

    rows = http("GET", f"{R}/collections/sink/records:count", None, su)
    log("sink rows after run:", rows)
    time.sleep(35)  # past idle_checkpoint
    log("idle after run", mem(pid))
    final = dict(**mem(pid), peak_rss_mb=proc_status(pid)["VmHWM"]/1024, data_mb=sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(data) for f in fs)/1e6)
    log("final", final)
finally:
    stop()

hw = dict(cpu=[l.split(":",1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")][0],
          threads=os.cpu_count(), mem_gb=round(os.sysconf("SC_PAGE_SIZE")*os.sysconf("SC_PHYS_PAGES")/2**30, 1),
          kernel=platform.release(), sha=subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, text=True).strip(),
          loadavg_at_end=os.getloadavg(), oha=subprocess.check_output(["oha", "--version"], text=True).strip())
cfg = dict(warm_s=a.warm, measure_s=a.secs, reps=a.reps, seed_rows=a.seed, server_cpus=SERVER_CPUS, client_cpus=CLIENT_CPUS, seed_rows_per_s=round(a.seed/seed_s))
json.dump(dict(hw=hw, cfg=cfg, results=results, final=final), open(os.path.join(out, "results.json"), "w"), indent=1)

md = [f"# Monolith load results {stamp}", "", f"git `{hw['sha']}` · {hw['cpu']} ({hw['threads']} threads, {hw['mem_gb']} GB) · kernel {hw['kernel']} · {hw['oha']}",
      f"server pinned cpus {SERVER_CPUS}, oha pinned {CLIENT_CPUS}; warm {a.warm}s, measure {a.secs}s x {a.reps}; seed {a.seed} rows; loopback, no Docker limits.", "",
      "| scenario | conns | units/s (median) | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | server CPU % | RSS MB | success |", "|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|"]
for n, r in results.items():
    m = lambda k: st.median(x[k] for x in r["reps"])
    md.append(f"| {n} | {r['conns']} | {m('rps'):.0f} | {m('p50'):.2f} | {m('p90'):.2f} | {m('p99'):.2f} | {m('p999'):.2f} | {m('max'):.1f} | {m('cpu'):.0f} | {m('rss_mb'):.0f} | {m('success')*100:.2f}% |")
open(os.path.join(out, "report.md"), "w").write("\n".join(md) + "\n")
print("\n".join(md)); print("OUT", out)
