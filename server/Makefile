.PHONY: help fmt fmt-check lint build test test-db check db-up db-stop migrate check-schema seed-admin api worker worker-once docker-build up down

help:
	@echo "fmt fmt-check lint build test test-db check"
	@echo "db-up db-stop migrate check-schema seed-admin api worker worker-once"
	@echo "docker-build up down"
	@echo "api-docs-check（Rust OpenAPI 契约与示例校验）"
	@echo "observability-up observability-down observability-check observability-smoke observability-fault-smoke"

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings

build:
	cargo build --workspace --locked

test:
	cargo test --workspace --locked

test-db:
	cargo test -p silva-api --test identity --locked -- --ignored

check: fmt-check lint build test

.PHONY: api-docs-check
api-docs-check:
	cargo test --locked -p silva-api --test openapi

db-up:
	docker compose up -d --wait postgres

db-stop:
	docker compose stop postgres

migrate:
	cargo run --locked -p silva-cli -- migrate

check-schema:
	cargo run --locked -p silva-cli -- check-schema

seed-admin:
	cargo run --locked -p silva-cli -- seed-admin

api:
	cargo run --locked -p silva-api

worker:
	cargo run --locked -p silva-worker

worker-once:
	cargo run --locked -p silva-worker -- --once

docker-build:
	docker build -t silva-backend:local .

up:
	docker compose --profile app up --build -d --wait

down:
	docker compose --profile app down

# 观测组件通过独立覆盖文件启用，普通开发启动不受影响。
OBS_COMPOSE = docker compose -f docker-compose.yaml -f docker-compose.observability.yaml --profile app --profile observability

.PHONY: observability-up observability-down observability-check observability-smoke
observability-up:
	$(OBS_COMPOSE) up --build -d --wait

observability-down:
	$(OBS_COMPOSE) down

observability-check:
	$(OBS_COMPOSE) run --rm --no-deps --entrypoint promtool prometheus check config /etc/prometheus/prometheus.yaml
	$(OBS_COMPOSE) run --rm --no-deps --entrypoint promtool prometheus test rules /etc/prometheus/alerts.test.yaml
	$(OBS_COMPOSE) run --rm --no-deps --entrypoint amtool alertmanager check-config /etc/alertmanager/alertmanager.yaml
	$(OBS_COMPOSE) run --rm --no-deps otel-collector validate --config=/etc/otel/config.yaml

observability-smoke:
	cargo run --locked -p silva-xtask -- smoke

.PHONY: observability-fault-smoke
observability-fault-smoke:
	cargo run --locked -p silva-xtask -- fault-smoke
