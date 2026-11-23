FROM rust:1.99.1-alpine3.23@sha256:51e1b72c7f236093c084f6e3051239f033fba9e4c0c1430cc2ed479d0a3bd14c
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
