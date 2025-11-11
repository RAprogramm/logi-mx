# AUR Submission Guide

This guide explains how to submit the logi-mx package to the Arch User Repository.

## Prerequisites

1. AUR account created at https://aur.archlinux.org
2. SSH key added to AUR account
3. Git configured with your name and email

## Steps to Submit

### 1. Clone AUR Repository

```bash
git clone ssh://aur@aur.archlinux.org/logi-mx.git aur-logi-mx
cd aur-logi-mx
```

### 2. Copy PKGBUILD

```bash
cp /home/ra/Projects/mouse/PKGBUILD .
```

### 3. Generate .SRCINFO

```bash
makepkg --printsrcinfo > .SRCINFO
```

### 4. Test Build

```bash
makepkg -sf
```

This will:
- Download source tarball from GitHub
- Verify sha256sum
- Build all binaries
- Run tests
- Create package

### 5. Test Installation

```bash
sudo pacman -U logi-mx-0.1.0-1-x86_64.pkg.tar.zst
```

Test the installed package:
```bash
# Check binaries
which logi-mx
which logi-mx-daemon
which logi-mx-ui

# Check udev rules
ls -l /usr/lib/udev/rules.d/90-logi-mx.rules

# Check systemd service
systemctl --user status logi-mx-daemon

# Test CLI
logi-mx info

# Test UI
logi-mx-ui
```

### 6. Commit and Push to AUR

```bash
git add PKGBUILD .SRCINFO
git commit -m "Initial release: logi-mx 0.1.0"
git push
```

## Package Information

- **Package name**: logi-mx
- **Version**: 0.1.0
- **License**: MIT
- **Dependencies**: hidapi, systemd, gtk4, libadwaita
- **Build dependencies**: cargo, rust>=1.91

## AUR Package Page

After submission, the package will be available at:
https://aur.archlinux.org/packages/logi-mx

## Installation for Users

After AUR submission, users can install with:

```bash
# Using paru
paru -S logi-mx

# Using yay
yay -S logi-mx

# Manual installation
git clone https://aur.archlinux.org/logi-mx.git
cd logi-mx
makepkg -si
```

## Post-Installation

Users should enable the daemon:
```bash
systemctl --user enable --now logi-mx-daemon
```

## Updating the Package

When releasing a new version:

1. Update version in Cargo.toml files
2. Create new git tag and GitHub release
3. Download new tarball and calculate sha256sum
4. Update PKGBUILD (pkgver and sha256sums)
5. Test build
6. Push to AUR with commit message: "Update to version X.Y.Z"

## Support

- GitHub Issues: https://github.com/RAprogramm/logi-mx/issues
- AUR Comments: https://aur.archlinux.org/packages/logi-mx
