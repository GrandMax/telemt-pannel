from datetime import datetime
from typing import Optional

from pydantic import BaseModel, Field


class UserCreate(BaseModel):
    username: str = Field(..., min_length=3, max_length=32, pattern=r"^[a-zA-Z0-9_]+$")
    data_limit: Optional[int] = None
    max_connections: Optional[int] = None
    max_unique_ips: Optional[int] = None
    expire_at: Optional[datetime] = None
    note: Optional[str] = None


class UserUpdate(BaseModel):
    data_limit: Optional[int] = None
    max_connections: Optional[int] = None
    max_unique_ips: Optional[int] = None
    expire_at: Optional[datetime] = None
    status: Optional[str] = None  # active, disabled, limited, expired
    note: Optional[str] = None


class UserResponse(BaseModel):
    id: int
    username: str
    secret: str
    status: str
    data_limit: Optional[int] = None
    data_used: int = 0
    max_connections: Optional[int] = None
    max_unique_ips: Optional[int] = None
    expire_at: Optional[datetime] = None
    note: Optional[str] = None
    created_at: datetime
    last_seen_at: Optional[datetime] = None
    active_unique_ips: Optional[int] = None
    proxy_links: Optional[dict[str, str]] = None  # tg_link, https_link when requested


class UserLinksResponse(BaseModel):
    tg_link: str
    https_link: str
