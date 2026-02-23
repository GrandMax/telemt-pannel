from pydantic import BaseModel


class HealthResponse(BaseModel):
    status: str = "ok"


class SystemStatsResponse(BaseModel):
    uptime: float = 0
    total_connections: int = 0
    bad_connections: int = 0


class TrafficPoint(BaseModel):
    time: str  # ISO hour
    octets_from: int = 0
    octets_to: int = 0


class TrafficResponse(BaseModel):
    hourly: list[TrafficPoint] = []
