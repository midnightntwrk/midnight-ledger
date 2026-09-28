FROM rust:1.98.0-alpine3.23@sha256:4743b6231029d726d7a0f81d730a7c9f4eff23225a4499c01e275efb5e260235
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
