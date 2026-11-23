#!/usr/bin/env bash
# Repeatable local security audit. Usage: deploy/security-audit.sh [repo-dir] [report-dir]
# Needs: gitleaks, semgrep, osv-scanner, trivy, zizmor, bandit (all free/open source).
set -uo pipefail
SRC=${1:-.}; OUT=${2:-security-reports}; mkdir -p "$OUT"; cd "$SRC"
rc=0; step(){ printf '== %s\n' "$1"; }
step gitleaks;    gitleaks git . --no-banner --redact -f sarif -r "$OUT/gitleaks.sarif" || rc=1
step semgrep;     semgrep scan --config p/default --config p/python --config p/rust --config p/secrets --config p/bash --metrics off --error -q --sarif -o "$OUT/semgrep.sarif" . || rc=1
step osv-scanner; osv-scanner scan source -r . --format sarif --output-file "$OUT/osv.sarif" >/dev/null || rc=1
step trivy;       trivy fs --scanners vuln,secret,misconfig -q --exit-code 1 --severity MEDIUM,HIGH,CRITICAL -f sarif -o "$OUT/trivy.sarif" . || rc=1
step zizmor;      zizmor --format sarif .github/workflows > "$OUT/zizmor.sarif" || rc=1
step bandit
# Prefer SARIF (needs bandit-sarif-formatter); otherwise fall back to json so the audit still completes.
if bandit --help 2>&1 | grep -qw sarif; then
  bandit -q -ll -r backend/compute_mgr npu -f sarif -o "$OUT/bandit.sarif" || rc=1
else
  echo "   bandit SARIF formatter missing -> json fallback ($OUT/bandit.json; install: pipx inject bandit bandit-sarif-formatter)"
  bandit -q -ll -r backend/compute_mgr npu -f json -o "$OUT/bandit.json" || rc=1
  python3 -c 'import json,sys; print("   bandit (json) issues >= medium:", len(json.load(open(sys.argv[1])).get("results", [])))' "$OUT/bandit.json" || rc=1
fi
python3 - "$OUT" <<'PY'
import json, pathlib, sys
out = pathlib.Path(sys.argv[1]); runs = []
for f in sorted(out.glob("*.sarif")):
    if f.name == "combined.sarif": continue
    try: runs += json.loads(f.read_text() or "{}").get("runs", [])
    except ValueError: pass
(out / "combined.sarif").write_text(json.dumps({"version": "2.1.0", "$schema": "https://json.schemastore.org/sarif-2.1.0.json", "runs": runs}))
print("results:", {r["tool"]["driver"]["name"]: len(r.get("results", [])) for r in runs})
PY
exit $rc
