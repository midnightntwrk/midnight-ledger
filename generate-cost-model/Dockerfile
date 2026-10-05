FROM rust:1.98.1-alpine3.23@sha256:94a43bda4fd9b71fa3a74621c0982a646f83c7d1a3ba52bb4b7ddfd76d4dca93
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
