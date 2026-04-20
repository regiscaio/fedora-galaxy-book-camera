FROM registry.fedoraproject.org/fedora:44

RUN dnf install -y \
    cargo \
    rust \
    rpm-build \
    gettext \
    pkgconf-pkg-config \
    clang \
    gcc \
    gcc-c++ \
    make \
    desktop-file-utils \
    gtk4-devel \
    libadwaita-devel \
    libcamera-devel \
    libX11-devel \
    libXcursor-devel \
    libXext-devel \
    libXi-devel \
    libXinerama-devel \
    libXrandr-devel \
    libxkbcommon-devel \
    mesa-libEGL-devel \
    mesa-libGL-devel \
    wayland-devel \
    && dnf clean all

ENV CARGO_HOME=/cargo
WORKDIR /workspace
