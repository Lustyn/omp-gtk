FROM fedora:44

RUN dnf install --assumeyes \
        alsa-lib-devel \
        cargo \
        fontconfig-devel \
        gcc \
        gtk4-devel \
        libadwaita-devel \
        pkgconf-pkg-config \
        python3 \
        rpm-build \
        rust \
    && dnf clean all
