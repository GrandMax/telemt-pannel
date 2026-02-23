import asyncio
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from app.database import engine, Base, SessionLocal
from app.routers import admin, user, system
from app.services.metrics_scraper import fetch_metrics, scrape_and_persist
from app.services.user_service import get_active_users_for_config
from app.services.config_writer import write_config
from app.config import settings

# When running in Docker, static/ is next to backend/ (parent of app/)
STATIC_DIR = Path(__file__).resolve().parent.parent.parent / "static"


async def _metrics_loop(app: FastAPI):
    last_values: dict[str, tuple[int, int]] = {}
    while True:
        await asyncio.sleep(30)
        if not settings.telemt_metrics_url:
            continue
        try:
            text = await fetch_metrics()
            db = SessionLocal()
            try:
                last_values = scrape_and_persist(db, text, last_values)
            finally:
                db.close()
        except Exception:
            pass  # log and continue


@asynccontextmanager
async def lifespan(app: FastAPI):
    Base.metadata.create_all(bind=engine)
    if settings.telemt_config_path:
        db = SessionLocal()
        try:
            write_config(get_active_users_for_config(db))
        finally:
            db.close()
    task = asyncio.create_task(_metrics_loop(app))
    yield
    task.cancel()
    try:
        await task
    except asyncio.CancelledError:
        pass


app = FastAPI(title="MTProxy Panel", lifespan=lifespan)
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(system.router)
app.include_router(admin.router)
app.include_router(user.router)

if STATIC_DIR.is_dir():
    app.mount("/", StaticFiles(directory=str(STATIC_DIR), html=True), name="static")
else:

    @app.get("/")
    def root():
        return {"service": "mtpannel-panel", "docs": "/docs"}
