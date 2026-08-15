FROM archlinux:base

RUN pacman --sync --refresh --noconfirm \
        base-devel \
        fontconfig \
        gtk4 \
        libadwaita \
        python \
        rust \
    && pacman --sync --clean --noconfirm
