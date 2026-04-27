FROM rust:1.94.0-alpine3.23@sha256:ff0adc35894eb79586ce752a1b5a9eadc88b938c56d8f2b4b537b6258ff3fa10
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
