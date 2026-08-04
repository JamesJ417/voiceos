"""Read-only installation control-center evidence and safeguarded action proposals."""
from __future__ import annotations
import json, os, platform, shutil, socket, subprocess, time
from pathlib import Path
from services.gateway.system_health import collect_system_health

SERVICES=("voiceos-gateway","voiceos-hermes","voiceos-hermes-skill-worker","voiceos-gpu-scheduler","ollama","tailscaled")

def command(argv:list[str],timeout:float=5)->dict[str,object]:
    executable=shutil.which(argv[0])
    if not executable:return {"available":False}
    try:
        result=subprocess.run([executable,*argv[1:]],capture_output=True,text=True,timeout=timeout,check=False)
        return {"available":True,"ok":result.returncode==0,"output":(result.stdout or result.stderr).strip()[:10000]}
    except (OSError,subprocess.SubprocessError) as error:return {"available":True,"ok":False,"error":str(error)}

def collect()->dict[str,object]:
    health=collect_system_health(Path.cwd())
    services={name:command(["systemctl","is-active",name]) for name in SERVICES}
    gpu=command(["nvidia-smi","--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu","--format=csv,noheader,nounits"])
    models=command(["ollama","ps"])
    audio={"sinks":command(["pactl","list","short","sinks"]),"sources":command(["pactl","list","short","sources"])}
    evaluations=Path("/var/lib/voiceos/evaluations")
    latest=max(evaluations.glob("*.json"),key=lambda path:path.stat().st_mtime,default=None) if evaluations.exists() else None
    backup={"last_restore_test":latest.stat().st_mtime if latest else None,"evidence_file":str(latest) if latest else None}
    return {"checked_at":time.time(),"host":socket.gethostname(),"platform":platform.platform(),"resources":health,"gpu":gpu,"models":models,"audio":audio,"services":services,"backup":backup,"failures":[name for name,value in services.items() if value.get("available") and not value.get("ok")]}

def action_proposal(action:str,target:str)->dict[str,object]:
    if action=="restart_service" and target in SERVICES:
        argv=["/usr/bin/systemctl","restart",target]; rollback=f"Inspect journalctl -u {target}; restart the prior service configuration if needed."
    elif action=="speaker_test":argv=["/usr/bin/speaker-test","-t","sine","-f","880","-l","1"];rollback="Audio test is transient; no rollback required."
    elif action=="microphone_test":argv=["/usr/bin/arecord","-d","3","-f","cd","/var/lib/voiceos/audio-test.wav"];rollback="Delete /var/lib/voiceos/audio-test.wav after review."
    else:raise ValueError("unsupported_administrator_action")
    return {"status":"approval_required","approval":{"tool":"rig.root_command","arguments":{"argv":argv,"cwd":"/opt/voiceos","timeout_seconds":120,"rollback":rollback},"single_use":True,"exact_effect":f"{action}:{target}"}}
