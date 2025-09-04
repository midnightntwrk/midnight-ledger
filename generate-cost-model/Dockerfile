FROM rust:1.89-bookworm
USER 1000:1000
WORKDIR /cost-model
COPY ./ ./
RUN chmod +x /cost-model/generate-cost-model/run-cost-model-docker.sh
ENTRYPOINT ["/cost-model/generate-cost-model/run-cost-model-docker.sh"]
