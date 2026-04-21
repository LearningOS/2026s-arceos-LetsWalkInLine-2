#!/usr/bin/env sh

set -eu

if [ "$#" -ne 1 ]; then
    printf "Usage: ./update_disk.sh [userapp path]\n"
    exit 1
fi

FILE="$1"

if [ ! -f "$FILE" ]; then
    printf "File '%s' doesn't exist!\n" "$FILE"
    exit 1
fi

if [ ! -f ./disk.img ]; then
    printf "disk.img doesn't exist! Please 'make disk_img'\n"
    exit 1
fi

if [ "$(id -u)" -eq 0 ]; then
    SUDO=""
elif command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
else
    printf "Need root privilege (run as root or install sudo).\n"
    exit 1
fi

cleanup() {
    if [ -d ./mnt ]; then
        $SUDO umount ./mnt >/dev/null 2>&1 || true
        rm -rf ./mnt
    fi
}

trap cleanup EXIT

printf "Write file '%s' into disk.img\n" "$FILE"

mkdir -p ./mnt
$SUDO mount ./disk.img ./mnt
$SUDO mkdir -p ./mnt/sbin
$SUDO cp "$FILE" ./mnt/sbin
