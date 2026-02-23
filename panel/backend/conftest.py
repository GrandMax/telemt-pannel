import os
import pytest
from fastapi.testclient import TestClient
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from sqlalchemy.pool import StaticPool

os.environ.setdefault("DATABASE_URL", "sqlite://")
os.environ.setdefault("SECRET_KEY", "test-secret-key")
os.environ.setdefault("PROXY_HOST", "proxy.example.com")
os.environ.setdefault("PROXY_PORT", "443")
os.environ.setdefault("TLS_DOMAIN", "example.com")

from app.database import Base, get_db
from app.main import app
from app.models.admin import Admin
from app.utils.auth import hash_password

TEST_DATABASE_URL = "sqlite:///:memory:"
engine = create_engine(TEST_DATABASE_URL, connect_args={"check_same_thread": False}, poolclass=StaticPool)
TestingSessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)


def override_get_db():
    db = TestingSessionLocal()
    try:
        yield db
    finally:
        db.close()


@pytest.fixture(scope="function")
def db():
    Base.metadata.create_all(bind=engine)
    db = TestingSessionLocal()
    try:
        yield db
    finally:
        db.close()
        Base.metadata.drop_all(bind=engine)


def _make_db_override(db_session):
    def _override():
        yield db_session
    return _override


@pytest.fixture(scope="function")
def client(db):
    app.dependency_overrides[get_db] = _make_db_override(db)
    with TestClient(app) as c:
        yield c
    app.dependency_overrides.clear()


@pytest.fixture
def sudo_admin(db):
    admin = Admin(
        username="sudo",
        hashed_password=hash_password("sudo-pass"),
        is_sudo=True,
    )
    db.add(admin)
    db.commit()
    db.refresh(admin)
    return admin
