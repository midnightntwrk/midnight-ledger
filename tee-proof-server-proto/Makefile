# Makefile for Midnight Proof Server Multi-Arch Docker Container

.PHONY: help build build-local build-push test run stop clean logs inspect-manifest

# Configuration
IMAGE_NAME ?= midnight/proof-server
VERSION ?= latest
REGISTRY ?= docker.io
PLATFORMS ?= linux/amd64,linux/arm64

help: ## Show this help message
	@echo "Midnight Proof Server - Multi-Architecture Docker Build"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

setup-buildx: ## Set up Docker Buildx for multi-arch builds
	@echo "Setting up Docker Buildx..."
	@docker buildx create --name multiarch-builder --driver docker-container --bootstrap --use || docker buildx use multiarch-builder
	@docker buildx inspect --bootstrap

build-local: setup-buildx ## Build for local platform only (fast)
	@echo "Building for local platform from workspace root..."
	@cd .. && docker buildx build \
		--platform $$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}') \
		--file tee-proof-server-proto/Dockerfile \
		--tag $(IMAGE_NAME):$(VERSION) \
		--load \
		.
	@echo "✓ Built $(IMAGE_NAME):$(VERSION) for local platform"

build: setup-buildx ## Build multi-arch images (no push)
	@echo "Building multi-arch images from workspace root..."
	@cd .. && docker buildx build \
		--platform $(PLATFORMS) \
		--file tee-proof-server-proto/Dockerfile \
		--tag $(REGISTRY)/$(IMAGE_NAME):$(VERSION) \
		--tag $(REGISTRY)/$(IMAGE_NAME):latest \
		.
	@echo "✓ Built multi-arch images (not pushed)"

build-push: setup-buildx ## Build and push multi-arch images to registry
	@echo "Building and pushing multi-arch images from workspace root..."
	@cd .. && docker buildx build \
		--platform $(PLATFORMS) \
		--file tee-proof-server-proto/Dockerfile \
		--tag $(REGISTRY)/$(IMAGE_NAME):$(VERSION) \
		--tag $(REGISTRY)/$(IMAGE_NAME):latest \
		--push \
		.
	@echo "✓ Built and pushed to $(REGISTRY)/$(IMAGE_NAME):$(VERSION)"

inspect-manifest: ## Inspect the multi-arch manifest
	@docker buildx imagetools inspect $(REGISTRY)/$(IMAGE_NAME):$(VERSION)

run: ## Run the proof server locally
	@echo "Starting proof server..."
	@docker run -d \
		--name midnight-proof-server \
		-p 6300:6300 \
		-e RUST_LOG=info \
		-e MIDNIGHT_NETWORK=preview \
		$(IMAGE_NAME):$(VERSION)
	@echo "✓ Proof server running on http://localhost:6300"
	@echo "  View logs: make logs"
	@echo "  Stop: make stop"

run-compose: ## Run with docker-compose
	@docker-compose -f docker-compose.proof-server.yml up -d
	@echo "✓ Proof server started with docker-compose"

stop: ## Stop the running proof server
	@docker stop midnight-proof-server 2>/dev/null || true
	@docker rm midnight-proof-server 2>/dev/null || true
	@echo "✓ Proof server stopped"

stop-compose: ## Stop docker-compose services
	@docker-compose -f docker-compose.proof-server.yml down
	@echo "✓ Docker-compose services stopped"

logs: ## View proof server logs
	@docker logs -f midnight-proof-server

logs-compose: ## View docker-compose logs
	@docker-compose -f docker-compose.proof-server.yml logs -f

test: ## Test the proof server health endpoint
	@echo "Testing proof server..."
	@curl -f http://localhost:6300/health && echo "✓ Health check passed" || echo "✗ Health check failed"

shell: ## Open a shell in the running container
	@docker exec -it midnight-proof-server /bin/bash

clean: stop ## Clean up containers and images
	@docker rmi $(IMAGE_NAME):$(VERSION) 2>/dev/null || true
	@docker builder prune -f
	@echo "✓ Cleaned up"

clean-all: clean ## Clean up everything including buildx builder
	@docker buildx rm multiarch-builder 2>/dev/null || true
	@echo "✓ Removed buildx builder"

# AWS Nitro Enclave targets
aws-nitro-build: ## Build Nitro Enclave Image File (run on EC2)
	@./scripts/aws-nitro-deploy.sh

aws-nitro-deploy: ## Deploy to AWS Nitro Enclave (run on EC2)
	@./scripts/aws-nitro-deploy.sh

# Development helpers
dev: build-local run ## Build locally and run
	@echo "✓ Development environment ready"

watch-logs: ## Watch logs with auto-restart
	@watch -n 2 docker logs --tail 50 midnight-proof-server

status: ## Show container status
	@docker ps -a | grep midnight-proof-server || echo "No proof server container found"

# Multi-Version Support (for network compatibility)
build-multi: ## Build both legacy and current versions
	@./scripts/build-multi-version.sh

run-legacy: ## Run legacy version (6.1.0 compatible)
	@echo "Starting legacy proof server (6.1.0-alpha.6 compatible)..."
	@docker stop midnight-proof-server 2>/dev/null || true
	@docker rm midnight-proof-server 2>/dev/null || true
	@docker run -d \
		--name midnight-proof-server \
		-p 6300:6300 \
		-e RUST_LOG=info \
		-e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
		midnight/proof-server:v1-legacy
	@echo "✓ Legacy proof server running on http://localhost:6300"
	@echo "  Compatible with preview network 6.1.0-alpha.6"

run-current: ## Run current version (6.2.0+ compatible)
	@echo "Starting current proof server (6.2.0-alpha.1 with domain separators)..."
	@docker stop midnight-proof-server 2>/dev/null || true
	@docker rm midnight-proof-server 2>/dev/null || true
	@docker run -d \
		--name midnight-proof-server \
		-p 6300:6300 \
		-e RUST_LOG=info \
		-e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
		midnight/proof-server:v1-current
	@echo "✓ Current proof server running on http://localhost:6300"
	@echo "  Compatible with future network 6.2.0+"

switch-to-current: stop run-current ## Switch from legacy to current version
	@echo "✓ Switched to current version"

switch-to-legacy: stop run-legacy ## Switch from current to legacy version
	@echo "✓ Switched to legacy version"

check-version: ## Check which version to use for current network
	@./scripts/check-proof-server-config.sh

# CI/CD
ci-build: ## Build for CI (used in GitHub Actions)
	@docker buildx build \
		--platform $(PLATFORMS) \
		--file Dockerfile.proof-server \
		--tag $(REGISTRY)/$(IMAGE_NAME):$(VERSION) \
		--cache-from type=gha \
		--cache-to type=gha,mode=max \
		--push \
		.
