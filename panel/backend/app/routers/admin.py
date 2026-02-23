from fastapi import APIRouter, Depends, HTTPException, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer, OAuth2PasswordRequestForm
from sqlalchemy.orm import Session

from app.database import get_db
from app.models.admin import Admin
from app.schemas.admin import AdminCreate, AdminResponse, Token
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
