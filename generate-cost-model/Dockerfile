FROM rust:1.91.1-alpine3.23@sha256:45c1c35cd364b8055e9e86f8ecd3e8c874b2dcb658d8a4f94b5d111aa0d651a2
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
