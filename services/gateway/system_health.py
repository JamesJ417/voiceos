"""Deterministic host health evidence; no model judgment occurs here."""

from __future__ import annotations

import ctypes
import os
import platform
import shutil
import socket
import sys
from datetime import UTC, datetime
from pathlib import Path


def _memory_bytes() -> tuple[int | None, int | None]:
    if sys.platform == "win32":
        class MemoryStatus(ctypes.Structure):
            _fields_ = [
                ("length", ctypes.c_ulong),
                ("memory_load", ctypes.c_ulong),
                ("total_physical", ctypes.c_ulonglong),
                ("available_physical", ctypes.c_ulonglong),
                ("total_page_file", ctypes.c_ulonglong),
                ("available_page_file", ctypes.c_ulonglong),
                ("total_virtual", ctypes.c_ulonglong),
                ("available_virtual", ctypes.c_ulonglong),
                ("available_extended_virtual", ctypes.c_ulonglong),
            ]

        status = MemoryStatus()
        status.length = ctypes.sizeof(MemoryStatus)
        if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            return status.total_physical, status.available_physical
        return None, None

    page_size = os.sysconf("SC_PAGE_SIZE") if hasattr(os, "sysconf") else None
    physical_pages = os.sysconf("SC_PHYS_PAGES") if hasattr(os, "sysconf") else None
    available_pages = os.sysconf("SC_AVPHYS_PAGES") if hasattr(os, "sysconf") else None
    if page_size and physical_pages and available_pages:
        return page_size * physical_pages, page_size * available_pages
    return None, None


def collect_system_health(root: Path | None = None) -> dict[str, object]:
    check_root = root or Path.cwd()
    disk = shutil.disk_usage(check_root)
    memory_total, memory_available = _memory_bytes()
    disk_free_percent = round((disk.free / disk.total) * 100, 1) if disk.total else 0.0
    memory_available_percent = (
        round((memory_available / memory_total) * 100, 1)
        if memory_total and memory_available is not None
        else None
    )
    issues: list[str] = []
    if disk_free_percent < 10:
        issues.append("disk_space_low")
    if memory_available_percent is not None and memory_available_percent < 5:
        issues.append("memory_low")

    return {
        "status": "healthy" if not issues else "degraded",
        "checked_at": datetime.now(UTC).isoformat(),
        "host": socket.gethostname(),
        "operating_system": platform.system(),
        "os_release": platform.release(),
        "architecture": platform.machine(),
        "logical_cpu_count": os.cpu_count(),
        "memory_total_bytes": memory_total,
        "memory_available_bytes": memory_available,
        "memory_available_percent": memory_available_percent,
        "disk_total_bytes": disk.total,
        "disk_free_bytes": disk.free,
        "disk_free_percent": disk_free_percent,
        "issues": issues,
    }


def summarize_system_health(evidence: dict[str, object]) -> str:
    status = evidence["status"]
    disk_free = evidence["disk_free_percent"]
    memory_free = evidence["memory_available_percent"]
    cpu_count = evidence["logical_cpu_count"]
    if status == "healthy":
        return (
            f"The gateway host is healthy. It has {disk_free} percent disk space free, "
            f"{memory_free} percent memory available, and {cpu_count} logical CPU cores."
        )
    issues = ", ".join(str(issue).replace("_", " ") for issue in evidence["issues"])
    return f"The gateway host is degraded. Detected issues: {issues}."
