FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        build-essential \
        ca-certificates \
        cargo \
        dpkg-dev \
        libadwaita-1-dev \
        libfontconfig1-dev \
        libgtk-4-dev \
        pkg-config \
        python3 \
        rustc \
    && rm -rf /var/lib/apt/lists/*
