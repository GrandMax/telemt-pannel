from datetime import datetime
from typing import Any

from pydantic import BaseModel, Field


class AdminCreate(BaseModel):
    username: str = Field(..., min_length=1, max_length=64)
    password: str = Field(..., min_length=1)
    is_sudo: bool = False


class AdminResponse(BaseModel):
    id: int
    username: str
    is_sudo: bool
    created_at: str

    class Config:
        from_attributes = True


class AdminInDB(AdminResponse):
    hashed_password: str


class Token(BaseModel):
    access_token: str
    token_type: str = "bearer"


class TokenForm(BaseModel):
    username: str
    password: str


class ExportSettings(BaseModel):
    proxy_host: str = Field(..., min_length=1)
    proxy_port: int = Field(..., ge=1, le=65535)
    tls_domain: str = Field(..., min_length=1)
    telemt_metrics_url: str | None = None
    telemt_ignore_time_skew: bool = False


class ExportUser(BaseModel):
    username: str = Field(..., min_length=1, max_length=32)
    secret: str = Field(..., pattern=r"^[0-9a-fA-F]{32}$")
    enabled: bool
    ip_limit: int | None = Field(default=None, ge=0)
    comment: str | None = None
    status: str | None = None
    data_limit: int | None = Field(default=None, ge=0)
    max_connections: int | None = Field(default=None, ge=0)
    expire_at: datetime | None = None


class ExportSnapshot(BaseModel):
    version: int = Field(..., ge=1)
    exported_at: datetime
    users: list[ExportUser] = Field(default_factory=list)
    settings: ExportSettings


class ImportSnapshotRequest(BaseModel):
    version: int = Field(..., ge=1)
    exported_at: datetime
    users: list[dict[str, Any]] = Field(default_factory=list)
    settings: ExportSettings


class ImportSkippedItem(BaseModel):
    username: str | None = None
    reason: str


class ImportReport(BaseModel):
    added: int = 0
    updated: int = 0
    skipped: list[ImportSkippedItem] = Field(default_factory=list)
