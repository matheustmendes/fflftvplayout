#!/usr/bin/env bash
# Setup script for the Claude Code cloud environment (Ubuntu 24.04, root).
# Installs what the backend needs to compile: FFmpeg 7+ with development
# headers, pkg-config, clang/libclang and ALSA headers. Same source as CI
# (.github/workflows/validate.yml). Safe to run more than once.
# Needs network access to api.launchpad.net and ppa.launchpadcontent.net.
set -euxo pipefail

export DEBIAN_FRONTEND=noninteractive
# add-apt-repository needs the Python that has apt_pkg (3.12 on Ubuntu 24.04).

apt-get update
apt-get install -y software-properties-common
/usr/bin/python3.12 /usr/bin/add-apt-repository -y ppa:ubuntuhandbook1/ffmpeg7
apt-get update
apt-get install -y \
    ffmpeg libavcodec-dev libavdevice-dev libavfilter-dev libavformat-dev \
    libavutil-dev libswresample-dev libswscale-dev \
    pkg-config clang libclang-dev libasound2-dev pandoc

ffmpeg -version | head -n 1
test "$(pkg-config --modversion libavformat | cut -d. -f1)" -ge 61
