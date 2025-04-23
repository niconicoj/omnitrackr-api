FROM rust:1.85.0-slim AS builder

WORKDIR /usr/src/omnitrackr-api
RUN apt-get update && apt-get install -y libclang-dev g++ cmake git wget
COPY . .
RUN wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx
RUN wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx.json
RUN cargo install --path .

FROM debian:stable-slim

WORKDIR /app

RUN apt-get update && apt-get install -y espeak-ng-data && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/omnitrackr-api /usr/local/bin/omnitrackr-api
COPY --from=builder /usr/src/omnitrackr-api/en_US-hfc_female-medium.onnx /app/en_US-hfc_female-medium.onnx
COPY --from=builder /usr/src/omnitrackr-api/en_US-hfc_female-medium.onnx.json /app/en_US-hfc_female-medium.onnx.json

EXPOSE 3000
ENV PIPER_ESPEAKNG_DATA_DIRECTORY=/usr/lib/x86_64-linux-gnu

CMD ["omnitrackr-api"]
