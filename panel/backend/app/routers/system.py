from datetime import datetime, timedelta, timezone
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session
from sqlalchemy import desc, func

from app.database import get_db
from app.models.admin import Admin
from app.models.stats import SystemStats, TrafficLog
from app.routers.admin import get_current_admin
from app.schemas.system import (
    HealthResponse,
    SystemStatsResponse,
    TrafficResponse,
    TrafficPoint,
)

router = APIRouter(tags=["system"])


@router.get("/health", response_model=HealthResponse)
def health():
    return HealthResponse()


@router.get("/api/system/stats", response_model=SystemStatsResponse)
def system_stats(
    db: Session = Depends(get_db),
    _admin: Admin = Depends(get_current_admin),
):
    row = db.query(SystemStats).order_by(desc(SystemStats.recorded_at)).first()
    if not row:
        return SystemStatsResponse()
    return SystemStatsResponse(
        uptime=row.uptime,
        total_connections=row.total_connections,
        bad_connections=row.bad_connections,
    )


@router.get("/api/system/traffic", response_model=TrafficResponse)
def traffic(
    db: Session = Depends(get_db),
    _admin: Admin = Depends(get_current_admin),
    hours: int = Query(24, ge=1, le=168),
):
    since = datetime.now(timezone.utc) - timedelta(hours=hours)
    # SQLite: group by hour using strftime
    sub = (
        db.query(
            func.strftime("%Y-%m-%dT%H:00:00", TrafficLog.recorded_at).label("hour"),
            func.sum(TrafficLog.octets_from).label("octets_from"),
            func.sum(TrafficLog.octets_to).label("octets_to"),
        )
        .filter(TrafficLog.recorded_at >= since)
        .group_by(func.strftime("%Y-%m-%dT%H:00:00", TrafficLog.recorded_at))
        .order_by("hour")
    )
    rows = sub.all()
    return TrafficResponse(
        hourly=[
            TrafficPoint(time=r.hour or "", octets_from=r.octets_from or 0, octets_to=r.octets_to or 0)
            for r in rows
        ]
    )
