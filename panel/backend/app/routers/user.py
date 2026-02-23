from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, status, Query
from sqlalchemy.orm import Session

from app.database import get_db
from app.models.admin import Admin
from app.models.user import User
from app.routers.admin import get_current_admin
from app.schemas.user import UserCreate, UserUpdate, UserResponse, UserLinksResponse
from app.utils.links import proxy_links
from app.services.user_service import (
    create_user as svc_create_user,
    sync_config,
    regenerate_secret as svc_regenerate_secret,
)

router = APIRouter(prefix="/api/users", tags=["users"])


def _user_to_response(u: User, include_links: bool = False) -> UserResponse:
    links = proxy_links(u.secret) if include_links else None
    return UserResponse(
        id=u.id,
        username=u.username,
        secret=u.secret,
        status=u.status,
        data_limit=u.data_limit,
        data_used=u.data_used or 0,
        max_connections=u.max_connections,
        max_unique_ips=u.max_unique_ips,
        expire_at=u.expire_at,
        note=u.note,
        created_at=u.created_at,
        proxy_links=links,
    )


@router.get("", response_model=dict)
def list_users(
    db: Session = Depends(get_db),
    admin: Admin = Depends(get_current_admin),
    offset: int = Query(0, ge=0),
    limit: int = Query(50, ge=1, le=200),
    search: Optional[str] = Query(None),
    status_filter: Optional[str] = Query(None, alias="status"),
):
    q = db.query(User)
    if search:
        q = q.filter(User.username.contains(search))
    if status_filter:
        q = q.filter(User.status == status_filter)
    total = q.count()
    users = q.order_by(User.username).offset(offset).limit(limit).all()
    return {"users": [_user_to_response(u) for u in users], "total": total}


@router.post("", response_model=UserResponse, status_code=status.HTTP_201_CREATED)
def create_user(
    body: UserCreate,
    db: Session = Depends(get_db),
    admin: Admin = Depends(get_current_admin),
):
    if db.query(User).filter(User.username == body.username).first():
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Username already exists")
    user = svc_create_user(
        db,
        body.username,
        data_limit=body.data_limit,
        max_connections=body.max_connections,
        max_unique_ips=body.max_unique_ips,
        expire_at=body.expire_at,
        note=body.note,
        created_by_admin_id=admin.id,
    )
    return _user_to_response(user, include_links=True)


@router.get("/{username}", response_model=UserResponse)
def get_user(
    username: str,
    db: Session = Depends(get_db),
    admin: Admin = Depends(get_current_admin),
):
    user = db.query(User).filter(User.username == username).first()
    if not user:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    return _user_to_response(user, include_links=True)


@router.get("/{username}/links", response_model=UserLinksResponse)
def get_user_links(
    username: str,
    db: Session = Depends(get_db),
    admin: Admin = Depends(get_current_admin),
):
    user = db.query(User).filter(User.username == username).first()
    if not user:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    links = proxy_links(user.secret)
    return UserLinksResponse(tg_link=links["tg_link"], https_link=links["https_link"])


@router.put("/{username}", response_model=UserResponse)
def update_user(
  username: str,
  body: UserUpdate,
  db: Session = Depends(get_db),
  admin: Admin = Depends(get_current_admin),
):
    user = db.query(User).filter(User.username == username).first()
    if not user:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    if body.data_limit is not None:
        user.data_limit = body.data_limit
    if body.max_connections is not None:
        user.max_connections = body.max_connections
    if body.max_unique_ips is not None:
        user.max_unique_ips = body.max_unique_ips
    if body.expire_at is not None:
        user.expire_at = body.expire_at
    if body.status is not None:
        user.status = body.status
    if body.note is not None:
        user.note = body.note
    db.commit()
    db.refresh(user)
    sync_config(db)
    return _user_to_response(user, include_links=True)


@router.delete("/{username}", status_code=status.HTTP_204_NO_CONTENT)
def delete_user(
  username: str,
  db: Session = Depends(get_db),
  admin: Admin = Depends(get_current_admin),
):
    user = db.query(User).filter(User.username == username).first()
    if not user:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    db.delete(user)
    db.commit()
    sync_config(db)


@router.post("/{username}/regenerate-secret", response_model=UserResponse)
def regenerate_secret(
  username: str,
  db: Session = Depends(get_db),
  admin: Admin = Depends(get_current_admin),
):
    user = db.query(User).filter(User.username == username).first()
    if not user:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    user = svc_regenerate_secret(db, user)
    return _user_to_response(user, include_links=True)
