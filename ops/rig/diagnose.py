"""Read-only VoiceOS rig diagnostics for Windows or Linux."""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen


def command(args: list[str], timeout: int = 10) -> dict[str, object]:
    executable = shutil.which(args[0])
    if executable is None:
        return {"available": False, "command": args[0]}
    try:
        completed = subprocess.run(
            [executable, *args[1:]],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as error:
        return {"available": True, "ok": False, "error": str(error)}
    output = (completed.stdout or completed.stderr).strip()
    return {
        "available": True,
        "ok": completed.returncode == 0,
        "exit_code": completed.returncode,
        "output": output[:20_000],
    }


def http_json(url: str) -> dict[str, object]:
    try:
        with urlopen(url, timeout=3) as response:
            return {"reachable": True, "status": response.status, "body": json.loads(response.read())}
    except (OSError, URLError, json.JSONDecodeError) as error:
        return {"reachable": False, "error": str(error)}


def memory_bytes() -> int | None:
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
                if line.startswith("MemTotal:"):
                    return int(line.split()[1]) * 1024
        except (OSError, ValueError, IndexError):
            return None
    return None


def collect() -> dict[str, object]:
    # Measure the host's root volume, not whichever staging or temporary
    # filesystem the diagnostic happens to be launched from.
    disk_root = Path(Path.cwd().anchor or os.sep)
    disk = shutil.disk_usage(disk_root)
    nvidia = command(
        [
            "nvidia-smi",
            "--query-gpu=name,memory.total,driver_version,temperature.gpu",
            "--format=csv,noheader,nounits",
        ]
    )
    ollama_cli = command(["ollama", "--version"])
    tailscale_cli = command(["tailscale", "version"])
    return {
        "host": platform.node(),
        "operating_system": platform.platform(),
        "architecture": platform.machine(),
        "logical_cpu_count": os.cpu_count(),
        "memory_total_bytes": memory_bytes(),
        "disk_total_bytes": disk.total,
        "disk_free_bytes": disk.free,
        "nvidia": nvidia,
        "ollama_cli": ollama_cli,
        "ollama_api": http_json("http://127.0.0.1:11434/api/version"),
        "ollama_models": http_json("http://127.0.0.1:11434/api/tags"),
        "tailscale_cli": tailscale_cli,
        "gateway": http_json("http://127.0.0.1:8787/v1/health"),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON")
    args = parser.parse_args()
    report = collect()
    if args.json:
        print(json.dumps(report, indent=2))
        return
    print(f"VoiceOS rig diagnostic for {report['host']}")
    print(f"OS: {report['operating_system']}")
    print(f"Architecture: {report['architecture']}")
    print(f"Logical CPUs: {report['logical_cpu_count']}")
    for label in ("nvidia", "ollama_cli", "tailscale_cli"):
        check = report[label]
        state = "OK" if check.get("ok") else "MISSING/FAILED"
        print(f"{label}: {state}")
        if check.get("output"):
            print(f"  {check['output']}")
    print(f"Ollama API: {'OK' if report['ollama_api'].get('reachable') else 'UNREACHABLE'}")
    print(f"Gateway: {'OK' if report['gateway'].get('reachable') else 'UNREACHABLE'}")


if __name__ == "__main__":
    main()
