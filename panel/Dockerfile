# Build context: panel/ (directory containing this Dockerfile)
# Stage 1: Build frontend
FROM node:20-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install
COPY frontend/ .
RUN npm run build

# Stage 2: Backend + serve frontend
FROM python:3.12-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -s /usr/sbin/nologin panel
COPY backend/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt

COPY backend/ ./backend
COPY --from=frontend /app/frontend/dist ./static

RUN mkdir -p /app/data /app/telemt-config \
    && chown panel:panel /app/data /app/telemt-config

WORKDIR /app/backend
ENV PYTHONPATH=/app/backend
EXPOSE 8080
USER panel
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8080"]
