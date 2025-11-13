# Quick Start: AUR Binary Package

## 🚀 Setup (One-time)

```bash
# 1. Create AUR repository
ssh aur@aur.archlinux.org setup-repo logi-mx-bin

# 2. Generate SSH key
ssh-keygen -t ed25519 -f ~/.ssh/aur_logi_mx_bin -C "logi-mx-bin-ci"

# 3. Add public key to AUR
cat ~/.ssh/aur_logi_mx_bin.pub
# → Paste at https://aur.archlinux.org/account/

# 4. Add secret to GitHub
gh secret set AUR_SSH_PRIVATE_KEY_BIN < ~/.ssh/aur_logi_mx_bin
```

## ✅ Done!

Next release will automatically publish both packages:
- `logi-mx` (source)
- `logi-mx-bin` (binary)

## 📦 Release Process

```bash
git tag v0.1.2
git push origin v0.1.2
```

CI automatically:
1. ✅ Builds binaries
2. ✅ Creates GitHub Release
3. ✅ Publishes to AUR (source)
4. ✅ Publishes to AUR (binary)

## 🔍 Verify

```bash
# Check workflow
gh run watch

# Check AUR
https://aur.archlinux.org/packages/logi-mx-bin
```

## 📊 CI Jobs

```
create-release        → Creates GitHub Release
  ↓
build-release         → Builds binaries
  ↓
publish-aur          → Publishes logi-mx (source)
publish-aur-bin      → Publishes logi-mx-bin (binary)
```

## 🎯 User Installation

```bash
# Fast (binary)
yay -S logi-mx-bin

# From source
yay -S logi-mx
```

Both provide the same functionality!
