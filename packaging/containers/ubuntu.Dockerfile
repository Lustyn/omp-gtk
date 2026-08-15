FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        build-essential \
        ca-certificates \
        cargo \
        dpkg-dev \
        libasound2-dev \
        libadwaita-1-dev \
        libfontconfig1-dev \
        libgtk-4-dev \
        lld \
        pkg-config \
        python3 \
        rustc \
    && rm -rf /var/lib/apt/lists/*

# Ubuntu's usr-merge transition leaves an x86-64 loader diversion that makes
# dpkg-shlibdeps distrust otherwise canonical libc6 ownership information.
RUN dpkg-divert --remove --no-rename /lib64/ld-linux-x86-64.so.2
