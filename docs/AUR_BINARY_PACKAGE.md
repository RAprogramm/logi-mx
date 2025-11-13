# AUR Binary Package Automation

## Overview

Automated CI/CD pipeline for publishing `logi-mx-bin` to AUR alongside the source package `logi-mx`.

## Architecture

```
GitHub Release (v0.1.1)
    ↓
Build binary archive (.tar.gz)
    ↓
Calculate SHA256
    ↓
Generate PKGBUILD.bin
    ↓
Generate .SRCINFO
    ↓
Publish to AUR (logi-mx-bin)
```

## Setup Instructions

### 1. Create AUR Repository

```bash
# Clone or create new AUR repository
ssh aur@aur.archlinux.org setup-repo logi-mx-bin
git clone ssh://aur@aur.archlinux.org/logi-mx-bin.git
```

### 2. Generate SSH Key for CI

```bash
# Generate dedicated SSH key for logi-mx-bin
ssh-keygen -t ed25519 -C "logi-mx-bin-ci" -f ~/.ssh/aur_logi_mx_bin

# Add public key to AUR account
cat ~/.ssh/aur_logi_mx_bin.pub
```

Go to https://aur.archlinux.org/account/ and add the public key.

### 3. Add GitHub Secret

```bash
# Add private key to GitHub Secrets
gh secret set AUR_SSH_PRIVATE_KEY_BIN < ~/.ssh/aur_logi_mx_bin
```

### 4. Verify Setup

```bash
# List secrets
gh secret list

# Expected output:
# AUR_SSH_PRIVATE_KEY      Updated 2024-XX-XX
# AUR_SSH_PRIVATE_KEY_BIN  Updated 2025-XX-XX
```

## CI Workflow

The `publish-aur-bin` job runs automatically on version tags:

1. **Download Release Archive**
   - Fetches binary archive from GitHub Release
   - URL: `https://github.com/RAprogramm/logi-mx/releases/download/v{VERSION}/logi-mx-{VERSION}-x86_64-unknown-linux-gnu.tar.gz`

2. **Calculate SHA256**
   - Computes checksum for verification
   - Ensures package integrity

3. **Generate PKGBUILD**
   - Uses `scripts/generate-pkgbuild-bin.sh`
   - Injects version and SHA256
   - Creates standardized package build file

4. **Generate .SRCINFO**
   - Creates AUR metadata
   - Uses Docker with Arch Linux container

5. **Publish to AUR**
   - Pushes to `ssh://aur@aur.archlinux.org/logi-mx-bin.git`
   - Automatic commit message with version

## Package Comparison

| Feature | logi-mx | logi-mx-bin |
|---------|---------|-------------|
| **Build Time** | ~5-10 min | <1 min |
| **Dependencies** | cargo, rust, build tools | Runtime only |
| **Size** | Compile locally | Download binary |
| **Use Case** | Development, custom builds | Fast installation |
| **AUR Votes** | Primary package | Convenience package |

## Manual Testing

```bash
# Test script locally
./scripts/generate-pkgbuild-bin.sh 0.1.1 <sha256sum>

# Build package
cd /tmp && mkdir test-bin && cd test-bin
cp /path/to/PKGBUILD.bin ./PKGBUILD
makepkg -si

# Verify installation
logi-mx --version
systemctl --user status logi-mx-daemon
```

## Troubleshooting

### SSH Authentication Failed

```bash
# Test SSH connection
ssh -T aur@aur.archlinux.org

# Check secret
gh secret list | grep AUR_SSH_PRIVATE_KEY_BIN
```

### SHA256 Mismatch

```bash
# Manually verify checksum
curl -sL "https://github.com/RAprogramm/logi-mx/releases/download/v0.1.1/logi-mx-0.1.1-x86_64-unknown-linux-gnu.tar.gz" | sha256sum
```

### PKGBUILD Generation Failed

```bash
# Test script with debug
bash -x scripts/generate-pkgbuild-bin.sh 0.1.1 <sha256>
```

## Maintenance

### Update Process

1. Create new git tag: `git tag v0.1.2`
2. Push tag: `git push origin v0.1.2`
3. CI automatically:
   - Builds release
   - Publishes `logi-mx` to AUR
   - Publishes `logi-mx-bin` to AUR

### Monitoring

```bash
# Check workflow status
gh run list --workflow=ci.yml

# View specific run
gh run view <run-id>

# Watch live
gh run watch
```

## Security

- **SSH Keys**: Separate keys for source and binary packages
- **Checksums**: SHA256 verification for all downloads
- **Secrets**: Stored in GitHub Secrets (encrypted)
- **Audit**: All commits signed and traceable

## References

- [AUR Submission Guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
- [PKGBUILD Reference](https://wiki.archlinux.org/title/PKGBUILD)
- [GitHub Actions Deploy AUR](https://github.com/KSXGitHub/github-actions-deploy-aur)
