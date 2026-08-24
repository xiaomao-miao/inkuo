# Multi-stage build: admin SPA -> .NET services
#
# All paths are relative to the build context, which is expected to be the
# `cloud-server/` directory at the project root.

# ---------- Stage 1a: Build the React admin SPA ----------
FROM node:20-alpine AS admin-frontend
WORKDIR /web

# Cache deps
COPY admin-frontend/package.json admin-frontend/pnpm-lock.yaml* ./
RUN corepack enable && corepack prepare pnpm@9.15.4 --activate && \
    pnpm install --no-frozen-lockfile

COPY admin-frontend/ ./
# Wipe the dist directory first — if a cached layer from a previous build
# left older hashed bundles behind, they'd persist alongside the new ones
# and confuse Vite's asset references.
RUN rm -rf dist && pnpm run build

# ---------- Stage 1b: Build the marketing landing site ----------
FROM node:20-alpine AS marketing-frontend
WORKDIR /web

# Cache deps
COPY marketing-frontend/package.json marketing-frontend/pnpm-lock.yaml* ./
RUN corepack enable && corepack prepare pnpm@9.15.4 --activate && \
    pnpm install --no-frozen-lockfile

COPY marketing-frontend/ ./
RUN rm -rf dist && pnpm run build

# ---------- Stage 2: Build .NET services ----------
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS dotnet-build
WORKDIR /dotnet

COPY src/Inkuso.Cloud.Core/*.csproj src/Inkuso.Cloud.Core/
COPY src/Inkuso.Cloud.Api/*.csproj src/Inkuso.Cloud.Api/
COPY src/Inkuso.Cloud.Billing/*.csproj src/Inkuso.Cloud.Billing/
COPY src/Inkuso.Cloud.Admin/*.csproj src/Inkuso.Cloud.Admin/
COPY tests/Inkuso.Cloud.Core.Tests/*.csproj tests/Inkuso.Cloud.Core.Tests/
COPY src/Inkuso.Cloud.slnx src/

RUN dotnet restore src/Inkuso.Cloud.slnx

COPY src/ src/

RUN dotnet publish src/Inkuso.Cloud.Api/Inkuso.Cloud.Api.csproj -c Release -o /app/api
RUN dotnet publish src/Inkuso.Cloud.Billing/Inkuso.Cloud.Billing.csproj -c Release -o /app/billing
RUN dotnet publish src/Inkuso.Cloud.Admin/Inkuso.Cloud.Admin.csproj -c Release -o /app/admin

# Copy the built React admin dist into wwwroot/admin/, and the marketing
# landing site straight into wwwroot/ so the root URL serves the marketing
# page (and asset paths like /logo.svg, /assets/* resolve naturally).
# Wipe any existing wwwroot first — without this, old hashed asset bundles
# from a previous build can linger alongside new ones, leaving the served
# index.html referring to hashes that no longer exist on disk.
RUN rm -rf /app/admin/wwwroot
COPY --from=admin-frontend /web/dist /app/admin/wwwroot/admin
COPY --from=marketing-frontend /web/dist /app/admin/wwwroot

# ------------------- API runtime -------------------
FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS api
WORKDIR /app
COPY --from=dotnet-build /app/api ./
EXPOSE 8080
ENV ASPNETCORE_URLS=http://+:8080
ENTRYPOINT ["dotnet", "Inkuso.Cloud.Api.dll"]

# ------------------- Billing runtime -------------------
FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS billing
WORKDIR /app
COPY --from=dotnet-build /app/billing ./
EXPOSE 8081
ENV ASPNETCORE_URLS=http://+:8081
ENV ASPNETCORE_ENVIRONMENT=Production
ENTRYPOINT ["dotnet", "Inkuso.Cloud.Billing.dll"]

# ------------------- Admin runtime -------------------
FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS admin
WORKDIR /app
COPY --from=dotnet-build /app/admin ./
# Ensure the upload directory exists at first boot — the operator can mount
# an actual volume here (see docker-compose.yml's `releases-data`) so the
# admin service can write uploaded installer binaries.
RUN mkdir -p /var/lib/inkuo/releases && chmod 755 /var/lib/inkuo/releases
EXPOSE 8082
ENV ASPNETCORE_URLS=http://+:8082
ENV ASPNETCORE_ENVIRONMENT=Production
ENTRYPOINT ["dotnet", "Inkuso.Cloud.Admin.dll"]
