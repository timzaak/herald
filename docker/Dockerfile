# Multi-stage build for Herald with frontend and backend
# Build order: backend first, then frontend (which depends on backend for API generation)

# Stage 1: Shared Rust build environment
FROM rust:1.90-alpine AS backend-chef
WORKDIR /app

# Install build dependencies for Rust compilation
RUN apk add --no-cache \
    musl-dev \
    postgresql-client \
    openssl-dev \
    openssl-libs-static \
    pkgconfig && \
    cargo install cargo-chef --locked

# Stage 2: Analyze backend dependency graph
FROM backend-chef AS backend-planner
COPY backend/ ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build backend
FROM backend-chef AS backend-builder
COPY --from=backend-planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --bin herald-app --recipe-path recipe.json

# Copy actual source files
COPY backend/ ./

# Build the actual backend binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --bin herald-app

# Stage 4: Build frontend (depends on backend for API generation)
FROM node:20-alpine AS frontend-builder
WORKDIR /frontend

# Install dependencies for frontend build
RUN apk add --no-cache openssl

# Copy the JS SDK first: the frontend depends on it via a `file:../sdk-web`
# link, and its dist/ + src/generated/ are (re)generated in-image below.
COPY sdk-web/ /sdk-web/

# Copy package files for dependency caching
COPY frontend/package*.json ./

# Install dependencies (SDK first — the frontend's file: link target must
# exist for npm ci to resolve the symlink)
RUN cd /sdk-web && npm ci --legacy-peer-deps
RUN npm ci --legacy-peer-deps

# Copy backend binary for API generation (create dummy if needed)
COPY --from=backend-builder /app/target/release/herald-app /tmp/herald-app

# Copy frontend source
COPY frontend/ ./

# Generate API types using the pre-built backend binary, then build the SDK
# (generates its src/generated + dist from the same exported spec), then the
# frontend bundle
RUN (/tmp/herald-app --export-openapi api.json && npx openapi-ts || echo "Warning: API generation failed, using existing types") && \
    (cd /sdk-web && /tmp/herald-app --export-openapi api.json && npx --no-install openapi-ts && npm run build) && \
    npm run build

# Stage 5: Final runtime image
FROM alpine:3.20

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libgcc \
    tzdata \
    wget \
    && addgroup -g 1000 herald \
    && adduser -D -u 1000 -G herald herald

WORKDIR /app

# Copy backend binary from builder
COPY --from=backend-builder /app/target/release/herald-app /app/herald-app

# Copy frontend static files
COPY --from=frontend-builder /frontend/dist /app/frontend/dist

# Copy migrations and production config
COPY backend/app/migrations /app/migrations
COPY backend/config/production.toml /app/config.toml

# Set proper permissions
RUN chown -R herald:herald /app && \
    chmod +x /app/herald-app

# Switch to non-root user
USER herald

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:3000/health || exit 1

# Config path — override at runtime by mounting a custom config or setting HERALD_CONFIG
ENV HERALD_CONFIG=/app/config.toml

# Run the server
CMD ["./herald-app"]
