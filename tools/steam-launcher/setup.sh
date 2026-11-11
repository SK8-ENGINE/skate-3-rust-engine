#!/bin/sh
# First-time setup for Linux and macOS (setup.ps1 is the Windows one): checks what this computer needs to build and
# play versions of this repository and installs what is missing. Doc: docs/hails-additions/44-every-os.md.
#   sh setup.sh             the check: one line per tool (id, name, ok 1/0, detail, required 1/0), tab-separated, as
#                           setup.ps1; exit 0 = every required tool is there
#   sh setup.sh --install   installs what is missing: the system packages in ONE step (asks for your password once,
#                           through sudo; Homebrew on macOS needs none), then Rust for this user (never as root)
# Package managers: apt (Debian, Ubuntu, Mint, Pop!_OS), dnf (Fedora), pacman (Arch, Manjaro, EndeavourOS), zypper
# (openSUSE), Homebrew (macOS). SKATE_LAUNCHER_RELEASES=1: prebuilt versions only, so the build tools are optional.
install=0
[ "${1:-}" = "--install" ] && install=1
os=$(uname)

has() { command -v "$1" >/dev/null 2>&1; }

pm=
if [ "$os" = Darwin ]; then
    pm=brew
    [ -x /opt/homebrew/bin/brew ] && PATH="/opt/homebrew/bin:$PATH"
    [ -x /usr/local/bin/brew ] && PATH="/usr/local/bin:$PATH"
else
    for p in apt-get dnf pacman zypper; do has "$p" && { pm=$p; break; }; done
fi
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# System packages per tool and package manager (empty: nothing to install that way).
pkgs() {
    case "$pm:$1" in
        apt-get:git|dnf:git|pacman:git|zypper:git|brew:git) echo git ;;
        apt-get:build) echo clang cmake pkg-config patchelf make ;;
        dnf:build) echo clang cmake pkgconf-pkg-config patchelf make ;;
        pacman:build) echo clang cmake pkgconf patchelf make ;;
        zypper:build) echo clang cmake pkg-config patchelf make ;;
        brew:build) echo cmake ;;
        apt-get:libs) echo libasound2-dev libudev-dev libvulkan-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev libxcb1-dev libxext-dev libxfixes-dev libxss-dev libxtst-dev ;;
        dnf:libs) echo alsa-lib-devel systemd-devel vulkan-loader-devel libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel libxcb-devel libXext-devel libXfixes-devel libXScrnSaver-devel libXtst-devel ;;
        pacman:libs) echo alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxi libxrandr libxcb libxext libxfixes libxss libxtst ;;
        zypper:libs) echo alsa-devel systemd-devel vulkan-devel libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel libxcb-devel libXext-devel libXfixes-devel libXss-devel libXtst-devel ;;
        apt-get:python) echo python3 python3-venv python3-tk python3-pip ;;
        dnf:python) echo python3 python3-tkinter python3-pip ;;
        pacman:python) echo python tk python-pip ;;
        zypper:python) echo python3 python3-tk python3-pip ;;
        brew:python) echo python@3.13 python-tk@3.13 ;;
        apt-get:vulkan|dnf:vulkan|zypper:vulkan) echo mesa-vulkan-drivers ;;
        pacman:vulkan) echo vulkan-icd-loader ;;
        brew:vulkan) echo molten-vk vulkan-loader ;;
        apt-get:gh|dnf:gh|zypper:gh|brew:gh) echo gh ;;
        pacman:gh) echo github-cli ;;
    esac
}

# Checks: print the detail and succeed when the tool is there.
check_git() { has git && git --version; }
check_build() {
    if [ "$os" = Darwin ]; then xcode-select -p >/dev/null 2>&1 && has cmake && echo "Xcode command line tools, $(cmake --version | head -1)"
    else has cc && has cmake && has pkg-config && has patchelf && echo "$(cmake --version | head -1), patchelf"; fi
}
check_libs() {
    [ "$os" = Darwin ] && { echo "not needed on macOS"; return 0; }
    has pkg-config && pkg-config --exists alsa libudev xkbcommon wayland-client x11 xcursor xi xrandr && echo "ALSA, udev, xkbcommon, Wayland, X11"
}
check_python() {
    for py in python3.13 python3; do
        has "$py" || continue
        "$py" -c 'import sys, tkinter, venv; assert sys.version_info >= (3, 11)' >/dev/null 2>&1 && { "$py" --version; return 0; }
    done
    return 1
}
check_rust() { has cargo && has rustup && rustup show active-toolchain 2>/dev/null | head -1 | grep . ; }
check_gh() { has gh && echo installed; }
check_vulkan() {
    if [ "$os" = Darwin ]; then
        for d in /opt/homebrew/lib /usr/local/lib; do [ -f "$d/libMoltenVK.dylib" ] && { echo "MoltenVK in $d"; return 0; }; done
        return 1
    fi
    { ldconfig -p 2>/dev/null | grep -q 'libvulkan.so.1'; } || ls /usr/lib*/libvulkan.so.1 /usr/lib/*/libvulkan.so.1 >/dev/null 2>&1 || return 1
    if has vulkaninfo; then vulkaninfo --summary >/dev/null 2>&1 && echo "Vulkan driver works" || { echo "Vulkan loader present, no working driver"; return 1; }
    else echo "Vulkan loader present"; fi
}

# id, name, required
tools='git	Git	1
build	C compiler, CMake and build helpers	1
libs	Linux libraries the game builds against	1
python	Python 3 with Tk (converts the game files)	1
rust	Rust (rustup, stable)	1
gh	GitHub CLI (optional: sending logs to GitHub)	0
vulkan	Vulkan (graphics driver; MoltenVK on macOS)	1'
if [ "${SKATE_LAUNCHER_RELEASES:-}" = 1 ]; then
    tools=$(printf '%s\n' "$tools" | awk -F'\t' 'BEGIN{OFS="\t"} $1=="build"||$1=="libs"||$1=="python"||$1=="rust"{$3=0} {print}')
fi

missing=
missing_req=0
tab=$(printf '\t')
while IFS="$tab" read -r id name req; do
    detail=$("check_$id" 2>/dev/null | head -1)
    if [ $? -eq 0 ] && [ -n "$detail" ] && "check_$id" >/dev/null 2>&1; then ok=1; else ok=0; detail=missing; missing="$missing $id"; [ "$req" = 1 ] && missing_req=1; fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$id" "$name" "$ok" "$detail" "$req"
done <<EOF
$tools
EOF

if [ $install = 0 ]; then exit $missing_req; fi
echo
if [ -z "$missing" ]; then echo '  Everything is installed.'; exit 0; fi

if [ -z "$pm" ]; then
    if [ "$os" = Darwin ]; then
        echo '  Homebrew is missing. Open https://brew.sh, copy the one line shown there into Terminal, press Enter,'
        echo '  then run this setup again.'
    else
        echo '  No supported package manager found (apt, dnf, pacman, zypper). Install the tools listed as missing with'
        echo "  your system's software centre, then run this setup again."
    fi
    exit 1
fi
if [ -r /etc/os-release ] && grep -q '^ID=steamos' /etc/os-release; then
    echo '  SteamOS keeps its system read-only, so packages cannot be installed here directly. Use the prebuilt'
    echo '  versions (they need only Vulkan, which SteamOS has), or set up a Distrobox container for building.'
fi

[ "$os" = Darwin ] && ! xcode-select -p >/dev/null 2>&1 && case " $missing " in *" build "*)
    echo '  Installing the Xcode command line tools: a window opens, click Install and wait for it to finish.'
    xcode-select --install 2>/dev/null ;;
esac

list=
for id in $missing; do [ "$id" = rust ] || list="$list $(pkgs "$id")"; done
list=$(echo $list)
if [ -n "$list" ]; then
    echo "  Installing:$( [ "$pm" = brew ] || echo ' (asks for your password once)') $list"
    case "$pm" in
        apt-get) sudo sh -c "apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q $list" ;;
        dnf) sudo dnf install -y -q $list ;;
        pacman) sudo pacman -S --needed --noconfirm $list ;;
        zypper) sudo zypper --non-interactive install $list ;;
        brew) brew install $list ;;
    esac || echo '  The package install did not finish (see the lines above).'
fi

case " $missing " in *" rust "*)
    echo '  Installing Rust for this user (rustup)...'
    if has curl; then curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable -q
    else echo '  curl is missing: install it, then run this setup again.'; fi
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    ;;
esac

case " $missing " in *" vulkan "*) [ "$os" != Darwin ] && \
    echo '  If Vulkan still fails: install or update the graphics driver (NVIDIA: the proprietary driver; AMD and Intel: Mesa).' ;;
esac

echo
left=
while IFS="$tab" read -r id name req; do
    [ "$req" = 1 ] && ! "check_$id" >/dev/null 2>&1 && left="$left, $name"
done <<EOF
$tools
EOF
if [ -z "$left" ]; then
    if [ "${SKATE_LAUNCHER_RELEASES:-}" = 1 ]; then echo '  All required tools are installed. Next: Play.'
    else echo '  All required tools are installed. Next: convert the game files (SETUP.sh) and build (BUILD.sh).'; fi
    exit 0
fi
echo "  Still missing: ${left#, }"
exit 1
