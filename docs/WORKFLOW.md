# Git Workflow Guide

This project uses a **two-branch workflow**: `dev` and `main`.

## Branch Structure

```
main    ← Production releases (stable)
  ↑
  └─ merge
dev     ← Development branch (integration)
```

## Branches

### `main` Branch
- **Purpose**: Production-ready code
- **Protected**: Yes (require pull requests)
- **Triggers**: GitHub Releases, AUR updates
- **Stability**: Stable, tested, release-ready

### `dev` Branch
- **Purpose**: Development and integration
- **Protected**: No (direct commits allowed)
- **Triggers**: CI tests only
- **Stability**: May contain unstable features

## Workflow

### Daily Development

```bash
# Start on dev branch
git checkout dev

# Create a feature branch
git checkout -b feature/your-feature-name

# Make changes and commit
git add .
git commit -m "feat: add your feature"

# Push to GitHub
git push origin feature/your-feature-name

# Create pull request to dev
# (via GitHub web interface)
```

### Merging to Production

```bash
# 1. Ensure dev is stable
git checkout dev
git pull origin dev

# 2. Run tests
cargo test --all

# 3. Merge to main
git checkout main
git merge dev

# 4. Create release tag
git tag -a v0.1.0 -m "Release v0.1.0"

# 5. Push both branches and tags
git push origin main
git push origin dev
git push origin v0.1.0
```

## Pull Request Process

### 1. Feature → dev

```bash
# From feature branch
git checkout dev
git pull origin dev
git checkout feature/your-feature
git rebase dev
git push origin feature/your-feature
```

Then create PR on GitHub: `feature/your-feature` → `dev`

### 2. dev → main (Release)

```bash
# Only maintainers do this
git checkout main
git pull origin main
git checkout dev
git pull origin dev
git checkout main
git merge dev --no-ff -m "Release v0.1.0"
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin main --tags
```

## Commit Message Convention

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: <description>

[optional body]

[optional footer]
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style (formatting, etc.)
- `refactor`: Code refactoring
- `test`: Adding/updating tests
- `chore`: Maintenance tasks
- `perf`: Performance improvements
- `ci`: CI/CD changes

**Examples:**
```bash
git commit -m "feat: add support for tool calling"
git commit -m "fix: resolve memory leak in streaming"
git commit -m "docs: update installation guide"
git commit -m "test: add integration tests for auth"
```

## Version Bumping

For releases, update version in `Cargo.toml`:

```toml
[package]
name = "aperture-router"
version = "0.1.0"  # Update this
```

Then commit before tagging:

```bash
git commit -m "chore: bump version to 0.1.0"
git tag -a v0.1.0 -m "Release v0.1.0"
```

## Hotfixes

For urgent fixes to main:

```bash
# Create hotfix branch from main
git checkout main
git checkout -b hotfix/critical-fix

# Make fix and test
# ...

# Merge back to main
git checkout main
git merge hotfix/critical-fix
git tag -a v0.1.1 -m "Hotfix: critical fix"

# Also merge to dev
git checkout dev
git merge hotfix/critical-fix

# Push everything
git push origin main dev --tags
```

## Branch Protection Rules (Recommended)

### `main` Branch
- ✅ Require pull request before merging
- ✅ Require status checks to pass
- ✅ Require branches to be up to date
- ❌ Do not allow bypassing settings

### `dev` Branch
- ⚠️ Optional: Require pull request for code review
- ✅ Require status checks to pass
- ✅ Allow direct commits (for convenience)

## CI/CD

- **On push to any branch**: Run tests
- **On pull request**: Run tests + clippy
- **On tag push to main**: Build release binaries
- **On release published**: Upload to GitHub Releases

## Troubleshooting

### "Merge conflict" when merging dev to main

```bash
# Resolve conflicts manually
git status
# Edit conflicted files
git add <resolved-files>
git commit
```

### "Branch is behind" error

```bash
# Update your branch
git pull origin dev --rebase
```

### Accidentally commit to main

```bash
# Create patch branch
git checkout main
git checkout -b patch/accidental-commit
git reset --hard dev

# Re-apply commits to dev instead
```

## Useful Commands

```bash
# See current branch
git branch --show-current

# See all branches
git branch -a

# See branch history
git log --oneline --graph --all

# Compare branches
git diff dev..main

# See unpushed commits
git log origin/dev..dev
```

## GitHub Setup

### 1. Create Repository

1. Go to GitHub → New repository
2. Name: `aperture-router`
3. Description: `Universal AI router for Tailscale Aperture`
4. Make it **Public**
5. **DO NOT** initialize with README (we have one)
6. Click "Create repository"

### 2. Push to GitHub

```bash
git remote add origin https://github.com/Wayazi/aperture-router.git
git push -u origin main
git push -u origin dev
```

### 3. Set Default Branch

On GitHub:
- Settings → Branches
- Set default branch to `dev`
- Add branch protection for `main`

### 4. Enable Branch Protection

For `main` branch:
- Settings → Branches → Add rule
- Branch name pattern: `main`
- ✅ Require a pull request before merging
- ✅ Require status checks to pass
- ✅ Require branches to be up to date
- ❌ Do not allow bypassing
