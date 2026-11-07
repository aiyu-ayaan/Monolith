#!/usr/bin/env python3
"""Idle footprint (doc 16 §1): cold start time, then RSS/threads with N apps open.

Usage: python3 bench/scenarios/footprint.py [--apps 20] [--rows 1000] [--bin target/release/monolith] [--cpus 0-3]
"""
import argparse, json, os, re, shutil, subprocess, tempfile, time, urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ap = argparse.ArgumentParser()
ap.add_argument("--apps", type=int, default=20)
ap.add_argument("--rows", type=int, default=1000)
ap.add_argument("--bin", default=os.path.join(ROOT, "target", "release", "monolith"))
ap.add_argument("--cpus", default="0-3")
a = ap.parse_args()
PORT = 18091
BASE = f"http://127.0.0.1:{PORT}"

def http(method, path, body=None, tok=None):
    r = urllib.request.Request(BASE + path, method=method, data=None if body is None else json.dumps(body).encode(),
        headers={"content-type": "application/json", **({"authorization": "Bearer " + tok} if tok else {})})
    with urllib.request.urlopen(r) as f:
        raw = f.read(); return json.loads(raw) if raw else None

def mem(pid):
    d = {}
    for l in open(f"/proc/{pid}/status"):
        k, _, v = l.partition(":")
        if k in ("VmRSS", "RssAnon", "RssFile", "Threads", "VmHWM"): d[k] = int(v.split()[0])
    return dict(rss_mb=round(d["VmRSS"]/1024, 1), anon_mb=round(d["RssAnon"]/1024, 1),
                file_mb=round(d["RssFile"]/1024, 1), threads=d["Threads"], peak_mb=round(d["VmHWM"]/1024, 1))

def cpu_s(pid):
    f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK")

def up():
    for _ in range(2000):
        try: http("GET", "/api/v1/system/health"); return
        except Exception: time.sleep(0.001)
    raise SystemExit("server did not start")

data = tempfile.mkdtemp(prefix="monolith-fp-")
env = {**os.environ, "MONOLITH_DATA": data, "MONOLITH_ADDR": f"127.0.0.1:{PORT}", "MONOLITH_LOG": "warn"}
out = {}
try:
    t0 = time.time()
    srv = subprocess.Popen(["taskset", "-c", a.cpus, a.bin], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    tok = None
    for line in srv.stdout:
        m = re.search(r"/install/([0-9a-f]+)", line)
        if m: tok = m.group(1); break
    up(); out["cold_start_first_run_ms"] = round((time.time() - t0) * 1000, 1)
    out["idle_fresh"] = mem(srv.pid)
    su = http("POST", "/api/v1/system/install", {"token": tok, "email": "fp@example.com", "password": "footprint-pass-123"})["token"]
    for i in range(a.apps):
        http("POST", "/api/v1/system/apps", {"name": f"app{i}"}, su)
        http("POST", f"/api/v1/apps/app{i}/collections", {"name": "items", "fields": [{"name": "title", "type": "text"}, {"name": "n", "type": "number"}], "rules": {"list": "", "view": ""}}, su)
        for b in range(0, a.rows, 500):
            http("POST", f"/api/v1/apps/app{i}/batch", {"ops": [{"op": "create", "collection": "items", "body": {"title": f"t{j}", "n": j}} for j in range(b, min(b + 500, a.rows))]}, su)
        http("GET", f"/api/v1/apps/app{i}/collections/items/records?limit=50")
        urllib.request.urlopen(BASE + f"/api/v1/apps/app{i}/buckets", timeout=5) if False else None
    out[f"apps_{a.apps}_after_load"] = mem(srv.pid)
    time.sleep(35)  # past the writers' idle checkpoint
    c0 = cpu_s(srv.pid); time.sleep(10)
    out[f"apps_{a.apps}_idle"] = mem(srv.pid)
    out["idle_cpu_pct_10s"] = round((cpu_s(srv.pid) - c0) / 10 * 100, 2)
    srv.terminate(); srv.wait(10)
    # restart with existing data: cold start and the cost of opening apps lazily
    t0 = time.time()
    srv = subprocess.Popen(["taskset", "-c", a.cpus, a.bin], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    up(); out["cold_start_restart_ms"] = round((time.time() - t0) * 1000, 1)
    out["restart_idle"] = mem(srv.pid)
    t0 = time.time(); http("GET", "/api/v1/apps/app0/collections/items/records?limit=1")
    out["first_request_open_app_ms"] = round((time.time() - t0) * 1000, 1)
    srv.terminate(); srv.wait(10)
finally:
    shutil.rmtree(data, ignore_errors=True)
print(json.dumps(out, indent=1))
