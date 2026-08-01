# AUR Package Update — v0.3.2

## Status: PENDING (AUR is down for maintenance)

The PKGBUILD and .SRCINFO have been updated for v0.3.2 but cannot be pushed to AUR because `aur.archlinux.org` is currently down for maintenance.

## When AUR is back

Run these commands to publish the update:

```bash
cd /tmp/aur-push
git init
git remote add origin ssh://aur@aur.archlinux.org/aperture-router.git
git fetch origin master
git checkout -b master origin/master

# Copy updated files
cp /home/x333/Wayazi/D3v/just_ev/aperture-router/aur/PKGBUILD .
cp /home/x333/Wayazi/D3v/just_ev/aperture-router/aur/.SRCINFO .
cp /home/x333/Wayazi/D3v/just_ev/aperture-router/aur/aperture-router.install .
cp /home/x333/Wayazi/D3v/just_ev/aperture-router/aur/README.md .

git add -A
git commit -m "chore: bump to 0.3.2"
git push origin master
```

## What changed in v0.3.2

- Version: 0.3.1 → 0.3.2
- sha256sum updated for new release tarball
- Fixes UTF-8 stream buffering (Stream interrupted errors)
- Includes complete docs overhaul (21 new doc files)

## Current AUR version: 0.3.1-3
## Target AUR version: 0.3.2-1
