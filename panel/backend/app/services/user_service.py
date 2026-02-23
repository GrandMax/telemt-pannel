"""User CRUD and config sync."""
from datetime import datetime, timezone
from typing import Optional

from sqlalchemy.orm import Session

from app.models.user import User
from app.services.config_writer import write_config, generate_secret_32hex


def get_active_users_for_config(db: Session) -> list[User]:
    now = datetime.now(timezone.utc)
    return (
        db.query(User)
        .filter(User.status == "active")
        .filter((User.expire_at.is_(None)) | (User.expire_at > now))
        .all()
    )


def sync_config(db: Session) -> None:
    users = get_active_users_for_config(db)
    write_config(users)


def create_user(
    db: Session,
    username: str,
    *,
    data_limit: Optional[int] = None,
    max_connections: Optional[int] = None,
    max_unique_ips: Optional[int] = None,
    expire_at: Optional[datetime] = None,
    note: Optional[str] = None,
    created_by_admin_id: Optional[int] = None,
) -> User:
    secret = generate_secret_32hex()
    user = User(
        username=username,
        secret=secret,
        status="active",
        data_limit=data_limit,
        data_used=0,
        max_connections=max_connections,
        max_unique_ips=max_unique_ips,
        expire_at=expire_at,
        note=note,
        created_by_admin_id=created_by_admin_id,
    )
    db.add(user)
    db.commit()
    db.refresh(user)
    sync_config(db)
    return user


def regenerate_secret(db: Session, user: User) -> User:
    user.secret = generate_secret_32hex()
    db.commit()
    db.refresh(user)
    sync_config(db)
    return user
