from sqlalchemy import BigInteger, Column, DateTime, ForeignKey, Integer, String, Text, func
from app.database import Base


class User(Base):
    __tablename__ = "users"

    id = Column(Integer, primary_key=True, autoincrement=True)
    username = Column(String(32), unique=True, nullable=False, index=True)
    secret = Column(String(32), nullable=False)  # 32 hex
    status = Column(String(32), default="active", nullable=False)  # active, disabled, limited, expired
    data_limit = Column(BigInteger, nullable=True)
    data_used = Column(BigInteger, default=0, nullable=False)
    max_connections = Column(Integer, nullable=True)
    max_unique_ips = Column(Integer, nullable=True)
    expire_at = Column(DateTime(timezone=True), nullable=True)
    note = Column(Text, nullable=True)
    created_at = Column(DateTime(timezone=True), server_default=func.now())
    created_by_admin_id = Column(Integer, ForeignKey("admins.id"), nullable=True)
