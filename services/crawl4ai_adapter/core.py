from __future__ import annotations

import hashlib
import ipaddress
import socket
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Callable, Iterable
from urllib.parse import urlsplit, urlunsplit


class RetrievalRejected(ValueError):
    """The requested retrieval violates the VoiceOS network policy."""


Resolver = Callable[[str, int], Iterable[str]]


@dataclass(frozen=True)
class RetrievalPolicy:
    max_pages: int = 8
    max_markdown_bytes: int = 2 * 1024 * 1024
    timeout_seconds: int = 30
    respect_robots_txt: bool = True


def system_resolver(hostname: str, port: int) -> list[str]:
    return sorted({entry[4][0] for entry in socket.getaddrinfo(hostname, port)})


def validate_public_url(url: str, resolver: Resolver = system_resolver) -> str:
    parsed = urlsplit(url.strip())
    if parsed.scheme not in {"http", "https"}:
        raise RetrievalRejected("only_http_and_https_are_allowed")
    if not parsed.hostname or parsed.username is not None or parsed.password is not None:
        raise RetrievalRejected("public_hostname_required")
    try:
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
    except ValueError as error:
        raise RetrievalRejected("invalid_port") from error
    if port not in {80, 443}:
        raise RetrievalRejected("only_standard_web_ports_are_allowed")
    try:
        addresses = list(resolver(parsed.hostname, port))
    except (OSError, socket.gaierror) as error:
        raise RetrievalRejected("hostname_resolution_failed") from error
    if not addresses:
        raise RetrievalRejected("hostname_resolution_failed")
    for address in addresses:
        ip = ipaddress.ip_address(address)
        if not ip.is_global:
            raise RetrievalRejected("private_or_reserved_network_denied")
    hostname = parsed.hostname.encode("idna").decode("ascii").lower()
    netloc = hostname if parsed.port is None else f"{hostname}:{port}"
    return urlunsplit((parsed.scheme.lower(), netloc, parsed.path or "/", parsed.query, ""))


def build_evidence(
    *,
    requested_url: str,
    final_url: str,
    markdown: str,
    links: list[str] | None = None,
    policy: RetrievalPolicy = RetrievalPolicy(),
) -> dict[str, object]:
    encoded = markdown.encode("utf-8")
    truncated = len(encoded) > policy.max_markdown_bytes
    if truncated:
        encoded = encoded[: policy.max_markdown_bytes]
        markdown = encoded.decode("utf-8", errors="ignore")
        encoded = markdown.encode("utf-8")
    return {
        "requested_url": requested_url,
        "final_url": final_url,
        "retrieved_at": datetime.now(UTC).isoformat(),
        "content_sha256": hashlib.sha256(encoded).hexdigest(),
        "markdown": markdown,
        "links": list(links or []),
        "truncated": truncated,
        "trust": "untrusted_external_content",
        "can_issue_instructions": False,
    }
