from __future__ import annotations

import os
from typing import Any

from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

from .core import RetrievalPolicy, RetrievalRejected, build_evidence, validate_public_url


class CrawlRequest(BaseModel):
    url: str = Field(min_length=1, max_length=4096)


app = FastAPI(title="VoiceOS Crawl4AI Adapter", docs_url=None, redoc_url=None)
policy = RetrievalPolicy(
    max_markdown_bytes=int(os.environ.get("VOICEOS_CRAWL_MAX_MARKDOWN_BYTES", 2 * 1024 * 1024)),
    timeout_seconds=int(os.environ.get("VOICEOS_CRAWL_TIMEOUT_SECONDS", 30)),
)


@app.get("/v1/health")
async def health() -> dict[str, object]:
    try:
        import crawl4ai  # noqa: F401

        configured = True
    except ImportError:
        configured = False
    return {"status": "ok" if configured else "degraded", "adapter": "crawl4ai", "configured": configured}


@app.post("/v1/retrieve")
async def retrieve(
    request: CrawlRequest,
    x_voiceos_device_id: str | None = Header(default=None),
) -> dict[str, object]:
    if not x_voiceos_device_id:
        raise HTTPException(status_code=401, detail="voiceos_device_identity_required")
    try:
        normalized_url = validate_public_url(request.url)
    except RetrievalRejected as error:
        raise HTTPException(status_code=400, detail=str(error)) from error
    try:
        from crawl4ai import AsyncWebCrawler, BrowserConfig, CacheMode, CrawlerRunConfig
    except ImportError as error:
        raise HTTPException(status_code=503, detail="crawl4ai_not_installed") from error

    browser_config = BrowserConfig(headless=True, verbose=False)
    run_config = CrawlerRunConfig(
        cache_mode=CacheMode.BYPASS,
        check_robots_txt=policy.respect_robots_txt,
        page_timeout=policy.timeout_seconds * 1000,
    )
    async with AsyncWebCrawler(config=browser_config) as crawler:
        result: Any = await crawler.arun(url=normalized_url, config=run_config)
    if not getattr(result, "success", False):
        raise HTTPException(status_code=502, detail="retrieval_failed")
    final_url = validate_public_url(str(getattr(result, "url", normalized_url)))
    markdown_value = getattr(result, "markdown", "")
    markdown = str(getattr(markdown_value, "raw_markdown", markdown_value) or "")
    links_value = getattr(result, "links", {}) or {}
    links = [
        str(item.get("href"))
        for group in links_value.values()
        if isinstance(group, list)
        for item in group
        if isinstance(item, dict) and item.get("href")
    ]
    return {"evidence": build_evidence(requested_url=request.url, final_url=final_url, markdown=markdown, links=links, policy=policy)}
