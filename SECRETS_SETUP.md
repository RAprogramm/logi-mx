# GitHub Secrets Setup

This document explains which secrets need to be configured in GitHub for CI/CD to work properly.

## Required Secrets

Navigate to: `https://github.com/RAprogramm/logi-mx/settings/secrets/actions`

### 1. AUR_SSH_PRIVATE_KEY (Required for AUR publishing)

This is your SSH private key for accessing AUR repository.

**How to generate:**

```bash
# Generate new SSH key for AUR
ssh-keygen -t ed25519 -C "andrey.rozanov.vl@gmail.com" -f ~/.ssh/aur
# Don't use a passphrase for CI

# Add public key to AUR account
cat ~/.ssh/aur.pub
# Copy the output and add it to https://aur.archlinux.org/account/

# Get private key for GitHub secret
cat ~/.ssh/aur
# Copy the ENTIRE output including:
# -----BEGIN OPENSSH PRIVATE KEY-----
# ...
# -----END OPENSSH PRIVATE KEY-----
```

**Add to GitHub:**
- Name: `AUR_SSH_PRIVATE_KEY`
- Value: Paste the entire private key content

### 2. CODECOV_TOKEN (Optional but recommended)

This enables code coverage reporting to Codecov.

**How to get:**

1. Go to https://codecov.io
2. Sign in with GitHub
3. Add repository: RAprogramm/logi-mx
4. Copy the upload token

**Add to GitHub:**
- Name: `CODECOV_TOKEN`
- Value: Paste the Codecov token

### 3. GITHUB_TOKEN (Automatically provided)

This token is automatically provided by GitHub Actions. No manual setup needed.

## Testing Secrets

After adding secrets, you can test them by:

1. **Test AUR publishing:**
   ```bash
   git tag v0.1.1
   git push origin v0.1.1
   ```
   This will trigger the release workflow and attempt to publish to AUR.

2. **Test coverage:**
   Push any commit to main branch, CI will run and upload coverage to Codecov.

## Security Notes

- Never commit private keys to the repository
- SSH keys for AUR should not have a passphrase (required for automated deployment)
- Rotate keys periodically
- Use separate SSH key only for AUR, not your personal GitHub key
- Secrets are encrypted and only exposed to GitHub Actions

## Workflow Permissions

Ensure GitHub Actions has proper permissions:

1. Go to: `https://github.com/RAprogramm/logi-mx/settings/actions`
2. Under "Workflow permissions", select:
   - "Read and write permissions"
   - Check "Allow GitHub Actions to create and approve pull requests"

This is required for:
- Creating releases
- Uploading release assets
- Dependabot PRs

## Troubleshooting

### AUR publishing fails

Check:
1. SSH key is correctly added to AUR account (public key)
2. SSH private key is correctly added to GitHub secrets (entire key including headers)
3. AUR account email matches commit_email in release.yml
4. You have write access to the AUR package

### Coverage upload fails

Check:
1. CODECOV_TOKEN is correctly set
2. Repository is added to Codecov
3. Token has not expired

### Release creation fails

Check:
1. GITHUB_TOKEN has write permissions
2. Tag follows semantic versioning (vX.Y.Z)
3. No release with same tag already exists
