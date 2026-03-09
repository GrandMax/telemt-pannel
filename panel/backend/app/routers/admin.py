from datetime import datetime, timezone

from fastapi import APIRouter, Depends, HTTPException, Query, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer, OAuth2PasswordRequestForm
from pydantic import ValidationError
from sqlalchemy.orm import Session

from app.config import settings
from app.database import get_db
from app.models.admin import Admin
from app.models.user import User
from app.schemas.admin import (
    AdminCreate,
    AdminResponse,
    ExportSettings,
    ExportSnapshot,
    ExportUser,
    ImportReport,
    ImportSkippedItem,
    ImportSnapshotRequest,
    Token,
)
from app.services.user_service import sync_config
from app.utils.auth import verify_password, hash_password, create_access_token, decode_access_token

router = APIRouter(prefix="/api/admin", tags=["admin"])
security = HTTPBearer(auto_error=False)


def get_current_admin(
    credentials: HTTPAuthorizationCredentials | None = Depends(security),
    db: Session = Depends(get_db),
) -> Admin:
    if not credentials or credentials.credentials is None:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Not authenticated")
    payload = decode_access_token(credentials.credentials)
    if not payload or "sub" not in payload:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid token")
    username = payload["sub"]
    admin = db.query(Admin).filter(Admin.username == username).first()
    if not admin:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Admin not found")
    return admin


def require_sudo(admin: Admin = Depends(get_current_admin)) -> Admin:
    if not admin.is_sudo:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Sudo required")
    return admin


def _export_settings() -> ExportSettings:
    return ExportSettings(
        proxy_host=settings.proxy_host,
        proxy_port=settings.proxy_port,
        tls_domain=settings.tls_domain,
        telemt_metrics_url=settings.telemt_metrics_url,
        telemt_ignore_time_skew=settings.telemt_ignore_time_skew,
    )


def _apply_settings(payload: ExportSettings) -> None:
    settings.proxy_host = payload.proxy_host
    settings.proxy_port = payload.proxy_port
    settings.tls_domain = payload.tls_domain
    settings.telemt_metrics_url = payload.telemt_metrics_url or ""
    settings.telemt_ignore_time_skew = payload.telemt_ignore_time_skew


def _user_enabled(user: User) -> bool:
    return user.status == "active"


def _user_status(payload: ExportUser) -> str:
    if payload.status:
        return payload.status
    return "active" if payload.enabled else "disabled"


def _user_to_export(user: User) -> ExportUser:
    return ExportUser(
        username=user.username,
        secret=user.secret,
        enabled=_user_enabled(user),
        ip_limit=user.max_unique_ips,
        comment=user.note,
        status=user.status,
        data_limit=user.data_limit,
        max_connections=user.max_connections,
        expire_at=user.expire_at,
    )


@router.post("/token", response_model=Token)
def login(form_data: OAuth2PasswordRequestForm = Depends(), db: Session = Depends(get_db)):
    admin = db.query(Admin).filter(Admin.username == form_data.username).first()
    if not admin or not verify_password(form_data.password, admin.hashed_password):
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid credentials")
    token = create_access_token(admin.username, {"is_sudo": admin.is_sudo})
    return Token(access_token=token)


@router.get("/me", response_model=AdminResponse)
def me(admin: Admin = Depends(get_current_admin)):
    return AdminResponse(
        id=admin.id,
        username=admin.username,
        is_sudo=admin.is_sudo,
        created_at=admin.created_at.isoformat() if admin.created_at else "",
    )


@router.post("", response_model=AdminResponse, status_code=status.HTTP_201_CREATED)
def create_admin(
    body: AdminCreate,
    db: Session = Depends(get_db),
    _: Admin = Depends(require_sudo),
):
    if db.query(Admin).filter(Admin.username == body.username).first():
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Username exists")
    admin = Admin(
        username=body.username,
        hashed_password=hash_password(body.password),
        is_sudo=body.is_sudo,
    )
    db.add(admin)
    db.commit()
    db.refresh(admin)
    return AdminResponse(
        id=admin.id,
        username=admin.username,
        is_sudo=admin.is_sudo,
        created_at=admin.created_at.isoformat() if admin.created_at else "",
    )


@router.get("/export", response_model=ExportSnapshot)
def export_snapshot(
    db: Session = Depends(get_db),
    _: Admin = Depends(require_sudo),
):
    users = db.query(User).order_by(User.username).all()
    return ExportSnapshot(
        version=1,
        exported_at=datetime.now(timezone.utc),
        users=[_user_to_export(user) for user in users],
        settings=_export_settings(),
    )


@router.post("/import", response_model=ImportReport)
def import_snapshot(
    body: ImportSnapshotRequest,
    mode: str = Query("merge"),
    db: Session = Depends(get_db),
    _: Admin = Depends(require_sudo),
):
    if mode not in {"merge", "replace"}:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Unsupported import mode")
    if body.version != 1:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Unsupported export version")

    _apply_settings(body.settings)

    parsed_users: list[ExportUser] = []
    skipped: list[ImportSkippedItem] = []
    for raw_user in body.users:
        username = raw_user.get("username") if isinstance(raw_user, dict) else None
        try:
            parsed_users.append(ExportUser.model_validate(raw_user))
        except ValidationError as exc:
            skipped.append(
                ImportSkippedItem(
                    username=str(username) if username else None,
                    reason=exc.errors()[0]["msg"],
                )
            )

    added = 0
    updated = 0

    if mode == "replace":
        db.query(User).delete()

    for payload in parsed_users:
        user = db.query(User).filter(User.username == payload.username).first()
        if user is None:
            user = User(
                username=payload.username,
                secret=payload.secret.lower(),
                status=_user_status(payload),
                data_limit=payload.data_limit,
                data_used=0,
                max_connections=payload.max_connections,
                max_unique_ips=payload.ip_limit,
                expire_at=payload.expire_at,
                note=payload.comment,
            )
            db.add(user)
            added += 1
            continue

        user.secret = payload.secret.lower()
        user.status = _user_status(payload)
        user.data_limit = payload.data_limit
        user.max_connections = payload.max_connections
        user.max_unique_ips = payload.ip_limit
        user.expire_at = payload.expire_at
        user.note = payload.comment
        updated += 1

    db.commit()
    sync_config(db)
    return ImportReport(added=added, updated=updated, skipped=skipped)
