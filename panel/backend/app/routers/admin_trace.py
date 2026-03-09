import logging
from urllib.parse import urlencode, urlsplit, urlunsplit

import httpx
from fastapi import APIRouter, Depends, HTTPException, Query

from app.config import settings
from app.models.admin import Admin
from app.routers.admin import require_sudo

router = APIRouter(prefix="/api/admin/trace", tags=["admin-trace"])


def _trace_base_url() -> str:
    metrics_url = settings.telemt_metrics_url.rstrip("/")
    if metrics_url.endswith("/metrics"):
        return metrics_url[:-8]
    return metrics_url


def _trace_url(path: str) -> str:
    base = _trace_base_url()
    parts = urlsplit(base)
    new_path = f"{parts.path.rstrip('/')}{path}"
    return urlunsplit((parts.scheme, parts.netloc, new_path, "", ""))


async def fetch_telemt_trace_json(path: str):
    url = _trace_url(path)
    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            response = await client.get(url)
            response.raise_for_status()
            return response.json()
    except httpx.HTTPStatusError as exc:
        status_code = exc.response.status_code
        if status_code == 404:
            raise HTTPException(status_code=404, detail="Trace session not found") from exc
        if status_code == 403:
            raise HTTPException(status_code=502, detail="telemt trace endpoint is not reachable from panel") from exc
        raise HTTPException(status_code=502, detail="telemt trace endpoint error") from exc
    except httpx.HTTPError as exc:
        raise HTTPException(status_code=502, detail="telemt trace endpoint unavailable") from exc


@router.get("/sessions")
async def trace_sessions(
    user: str | None = Query(None),
    dc: int | None = Query(None),
    limit: int = Query(50, ge=1, le=500),
    admin: Admin = Depends(require_sudo),
):
    logging.info("admin_trace_sessions username=%s user_filter=%s dc_filter=%s limit=%s", admin.username, user, dc, limit)
    payload = await fetch_telemt_trace_json(f"/trace/sessions?{urlencode({'limit': limit})}")
    sessions = payload.get("sessions", [])
    if user is not None:
        sessions = [session for session in sessions if session.get("user") == user]
    if dc is not None:
        sessions = [session for session in sessions if session.get("target_dc") == dc]
    return {"sessions": sessions}


@router.get("/{conn_id}")
async def trace_session_detail(
    conn_id: int,
    limit: int = Query(200, ge=1, le=1000),
    admin: Admin = Depends(require_sudo),
):
    logging.info("admin_trace_detail username=%s conn_id=%s limit=%s", admin.username, conn_id, limit)
    return await fetch_telemt_trace_json(f"/trace/{conn_id}?{urlencode({'limit': limit})}")
