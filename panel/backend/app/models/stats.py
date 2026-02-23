from sqlalchemy import BigInteger, Column, DateTime, Float, ForeignKey, Integer, func
from app.database import Base


class TrafficLog(Base):
    __tablename__ = "traffic_logs"

    id = Column(Integer, primary_key=True, autoincrement=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    octets_from = Column(BigInteger, default=0, nullable=False)
    octets_to = Column(BigInteger, default=0, nullable=False)
    recorded_at = Column(DateTime(timezone=True), server_default=func.now())


class SystemStats(Base):
    __tablename__ = "system_stats"

    id = Column(Integer, primary_key=True, autoincrement=True)
    uptime = Column(Float, default=0, nullable=False)
    total_connections = Column(Integer, default=0, nullable=False)
    bad_connections = Column(Integer, default=0, nullable=False)
    recorded_at = Column(DateTime(timezone=True), server_default=func.now())
