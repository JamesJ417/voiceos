"""Create a local, one-time VoiceOS enrollment QR code."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
from urllib.parse import quote

from .audit import AuditStore


def build_enrollment_uri(gateway_url: str, code: str) -> str:
    return (
        "voiceos://enroll?gateway="
        f"{quote(gateway_url.rstrip('/'), safe='')}"
        f"&code={quote(code, safe='')}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway", required=True, help="Private HTTPS gateway URL")
    parser.add_argument("--output", type=Path, required=True, help="Destination PNG file")
    parser.add_argument("--ttl", type=int, default=600, help="Validity in seconds")
    parser.add_argument("--data-dir", type=Path, default=None)
    args = parser.parse_args()
    if not args.gateway.startswith("https://"):
        raise SystemExit("The gateway must use HTTPS.")
    try:
        import qrcode
    except ImportError as error:
        raise SystemExit(
            "QR support is not installed. Run: "
            "python -m pip install -r services/gateway/requirements-enrollment.txt"
        ) from error

    data_dir = args.data_dir or Path(
        os.environ.get("VOICEOS_DATA_DIR", "work/gateway-data")
    )
    store = AuditStore(data_dir / "audit.sqlite3")
    try:
        code, expires_at = store.create_enrollment_code(args.ttl)
        uri = build_enrollment_uri(args.gateway, code)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        image = qrcode.make(uri)
        image.save(args.output)
        store.record_event(
            "enrollment.created",
            {"gateway_url": args.gateway, "expires_at": expires_at},
            actor="local-administrator",
        )
    finally:
        store.close()
    print(f"Enrollment QR: {args.output.resolve()}")
    print(f"Expires at Unix time: {expires_at}")


if __name__ == "__main__":
    main()
