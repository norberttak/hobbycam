# Releasing HobbyCAM

Maintainer guide. Written 2026-10-27; Apple's tools and portals change, so check the wording of menus against what you see.

Nothing in this repository pushes, tags, uploads or publishes by itself. Every step below that talks to GitHub or Apple is
run by you.

Overview of what the automation does:

| workflow / script | trigger | result |
|---|---|---|
| `.github/workflows/ci.yml` | push to `main`, pull request, manual | fmt, clippy, tests (macOS + Windows), actionlint + shellcheck, licence gate, manual build; on `main` the manual is deployed to GitHub Pages |
| `.github/workflows/release.yml` | tag `vX.Y.Z`, or manual (dry run) | Windows `.zip`, macOS `.dmg` (signed + notarized when the secrets exist), `SHA256SUMS`, a **draft** GitHub Release (tags only) |
| `scripts/macos-build.sh` | local or CI | `target/macos/HobbyCAM.app` (unsigned) |
| `scripts/macos-dmg.sh` | local or CI | signs the app, `dist/HobbyCAM-<V>-macos-arm64[-unsigned].dmg`, notarizes |
| `scripts/macos-appstore.sh` | local only | `dist/HobbyCAM-<V>-appstore.pkg` for the Mac App Store |
| `scripts/make-icons.sh` | local, when the icon design changes | `app/assets/icon/*` |

## 1. Create the GitHub repository (once)

1. Make sure the reference screenshots of other hobby-level CAM software and your scratch drawings are not in git. This must print nothing:

   ```sh
   git ls-files images drawings
   ```

2. Create an empty **public** repository `norberttak/hobbycam` (no README, no licence, no .gitignore), either on
   github.com → **New repository**, or with the GitHub CLI:

   ```sh
   gh repo create norberttak/hobbycam --public --source . --remote origin
   ```

   (On the web: afterwards run `git remote add origin https://github.com/norberttak/hobbycam.git`.)
3. Push: `git push -u origin main`.

## 2. Repository settings (once)

1. **Settings → Pages → Build and deployment → Source: GitHub Actions.**
   The `docs` job deploys only on pushes to `main`; if the first push ran before Pages was enabled, re-run the CI
   workflow (Actions → CI → Run workflow). The manual then appears at <https://norberttak.github.io/hobbycam/>.
2. **Settings → Actions → General → Workflow permissions: Read repository contents** (the jobs request more where they
   need it: Pages deploy, release draft).
3. Check the first CI run is green on macOS and Windows (the Windows icon resource and static CRT are first built there).

## 3. Apple Developer ID signing (once, renew when the certificate expires)

You need a paid Apple Developer Program membership.

1. **Developer ID Application certificate.** Either Xcode → Settings → Accounts → (your team) → **Manage Certificates** →
   **+** → *Developer ID Application*, or developer.apple.com → Certificates → **+** → *Developer ID Application* with a
   certificate signing request from Keychain Access (Certificate Assistant → Request a Certificate From a Certificate
   Authority → Saved to disk).
2. Export it with its private key: Keychain Access → login → My Certificates → right-click
   "Developer ID Application: Norbert Takacs (TEAMID)" → **Export** → `DeveloperID.p12`, choose a strong password.
3. **App Store Connect API key** (used by `notarytool`, avoids your Apple ID password and 2FA in CI):
   App Store Connect → Users and Access → **Integrations** → App Store Connect API → **Team Keys** → **+**,
   role **Developer**. Download `AuthKey_XXXXXXXXXX.p8` (only possible once), note the **Key ID** and the **Issuer ID**.
4. Add six repository secrets (Settings → Secrets and variables → Actions → **New repository secret**):

   | name | value |
   |---|---|
   | `MACOS_CERTIFICATE_P12_BASE64` | output of `base64 -i DeveloperID.p12 \| pbcopy` (pasted) |
   | `MACOS_CERTIFICATE_PASSWORD` | the .p12 export password |
   | `MACOS_SIGNING_IDENTITY` | `Developer ID Application: Norbert Takacs (TEAMID)` (exactly as in `security find-identity -v -p codesigning`) |
   | `APPSTORE_API_KEY_P8_BASE64` | output of `base64 -i AuthKey_XXXXXXXXXX.p8 \| pbcopy` |
   | `APPSTORE_API_KEY_ID` | the 10-character key id |
   | `APPSTORE_API_ISSUER_ID` | the issuer UUID |

   With the gh CLI instead: `base64 -i DeveloperID.p12 | gh secret set MACOS_CERTIFICATE_P12_BASE64` (and likewise).
   Delete the local `.p12`/`.p8` copies you no longer need, or keep them in a password manager.

Without any of the six secrets (e.g. in a fork) the release still builds: the app is ad-hoc signed and the file is named
`…-unsigned.dmg`. With only **some** of them set the macOS job fails and lists the missing names.

## 4. Dry run (before every first release with new secrets)

1. Actions → **Release** → **Run workflow** (branch `main`). It builds and uploads workflow artifacts; it never creates a
   release.
2. Download the `macos` and `windows` artifacts from the run page and check the dmg:

   ```sh
   spctl -a -t open --context context:primary-signature -vv HobbyCAM-0.1.0-macos-arm64.dmg
   # expect: accepted, source=Notarized Developer ID
   xcrun stapler validate HobbyCAM-0.1.0-macos-arm64.dmg
   ```

## 5. Release

1. Bump `version` in `[workspace.package]` of the root `Cargo.toml` (the only place; the tag must equal it).
2. Add a `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md` (with the real date; for 0.1.0 replace `YYYY-MM-DD`).
   This section becomes the release notes.
3. `cargo test --workspace` (the `release_check` tests also check that the changelog has the section).
4. Commit, then tag and push:

   ```sh
   git commit -am "Release X.Y.Z"
   git tag -a vX.Y.Z -m "HobbyCAM X.Y.Z"
   git push origin main vX.Y.Z
   ```

5. Wait for Actions → Release to finish. Open Releases → the **draft** → check the notes and the three files
   (`.zip`, `.dmg`, `SHA256SUMS`) → **Publish release**.

## 6. Verify on clean machines

- **Windows 10/11 without Rust or Visual C++ runtime:** download the zip in a browser, Extract All, run `hobbycam.exe`;
  SmartScreen: More info → Run anyway; the exe shows the icon in Explorer; open the sample, export G-code.
- **Apple-silicon Mac:** download the dmg in a browser (so it gets the quarantine flag), open it, drag to Applications,
  start it: no warning dialog. Check `sha256sum -c SHA256SUMS` / `shasum -a 256 -c SHA256SUMS`.

## 7. Mac App Store

The App Store build is made on your Mac by `scripts/macos-appstore.sh` (no CI): it builds with the `appstore` feature
(sandbox: Export G-code asks for a folder, no `.bak` files next to user files), embeds the provisioning profile, signs with
the sandbox entitlements and creates a signed installer package. It never uploads.

### One-time setup

1. developer.apple.com → Certificates, Identifiers & Profiles → **Identifiers** → **+** → App IDs → App → description
   "HobbyCAM", **Explicit** bundle id `io.github.norberttak.hobbycam` (it can never change once the App Store record
   exists).
2. Certificates → **+**: **Apple Distribution**, and **Mac Installer Distribution** (shows up in the keychain as
   "3rd Party Mac Developer Installer: …"). Install both into the login keychain (double-click).
3. Profiles → **+** → Distribution → **Mac App Store Connect** → the App ID above → the Apple Distribution certificate →
   download `HobbyCAM_AppStore.provisionprofile`.
4. App Store Connect → Apps → **+** → New App: platform **macOS**, name **HobbyCAM** (if the name is taken use e.g.
   "HobbyCAM 2D"), primary language, bundle id `io.github.norberttak.hobbycam`, SKU `hobbycam`, full access.

### Each App Store version

1. Build the package (find the identities with `security find-identity -v`):

   ```sh
   APPSTORE_TEAM_ID=ABCDE12345 \
   APPSTORE_APP_IDENTITY="Apple Distribution: Norbert Takacs (ABCDE12345)" \
   APPSTORE_INSTALLER_IDENTITY="3rd Party Mac Developer Installer: Norbert Takacs (ABCDE12345)" \
   APPSTORE_PROFILE=~/Downloads/HobbyCAM_AppStore.provisionprofile \
   scripts/macos-appstore.sh [BUILD]
   ```

   Always pass `BUILD` (CFBundleVersion) explicitly: a plain integer that goes up with every upload (`1`, `2`, `3` …),
   also for the same version (e.g. build `1` was rejected → upload again with `2`). Never reuse or lower a build number.
2. **Local sandbox smoke test** (before uploading). The App Store–signed app does not start outside the App Store or
   TestFlight, so test an ad-hoc signed copy with the sandbox switched on. The two identifier keys are removed from the
   entitlements because an ad-hoc signature cannot carry them (the app would be killed at launch):

   ```sh
   rm -rf dist/smoke && mkdir -p dist/smoke
   cp -R target/macos/HobbyCAM.app dist/smoke/
   cp packaging/macos/appstore.entitlements dist/smoke/smoke.entitlements
   /usr/libexec/PlistBuddy -c "Delete :com.apple.application-identifier" \
     -c "Delete :com.apple.developer.team-identifier" dist/smoke/smoke.entitlements
   codesign --force --sign - --entitlements dist/smoke/smoke.entitlements dist/smoke/HobbyCAM.app
   codesign -d --entitlements - dist/smoke/HobbyCAM.app   # must show com.apple.security.app-sandbox
   open dist/smoke/HobbyCAM.app
   ```

   Check, in a folder of your choice: new project → **Save** (no error); change something → **Save** again (no `.bak`
   next to the project); **Save As** under a new name; **Export G-code** into a folder (all segment files written);
   quit, start again, reopen the project; the tool library is still there; Help → User manual opens the browser.

   This creates the sandbox container `~/Library/Containers/io.github.norberttak.hobbycam`. Before installing the
   TestFlight build, quit the app and remove it so the TestFlight build starts with its own, correctly signed container:

   ```sh
   rm -rf ~/Library/Containers/io.github.norberttak.hobbycam dist/smoke
   ```

   (If `rm` reports "Operation not permitted", give Terminal Full Disk Access in System Settings → Privacy & Security,
   or delete the folder in Finder.)
3. **Upload** with **Transporter** (free, from the Mac App Store; the recommended way): sign in with your Apple ID, drag
   `dist/HobbyCAM-<V>-appstore.pkg` into the window, **Deliver**.
   CLI alternative (check `xcrun altool --help` on your Xcode version first; Apple changes these tools):

   ```sh
   # key file in ~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8
   xcrun altool --upload-app -f dist/HobbyCAM-0.1.0-appstore.pkg -t macos --apiKey <KEYID> --apiIssuer <ISSUER>
   ```

   altool's *notarization* commands were retired in November 2023 (`notarytool` replaced them); if altool reports that
   uploading is unavailable, use Transporter. `notarytool` cannot upload to the App Store.
4. **TestFlight test:** when the build has finished processing, App Store Connect → the app → **TestFlight** → add
   yourself as an internal tester, install the build from the TestFlight app and repeat the checks of step 2 with the
   real signature.
5. **Listing** (App Store Connect → the app → the macOS version):
   - subtitle (e.g. "2D CAM for hobby CNC mills"), description, keywords (cnc, cam, gcode, dxf, linuxcnc, milling);
   - category **Graphics & Design**;
   - support URL `https://github.com/norberttak/hobbycam/issues`;
   - privacy policy URL `https://github.com/norberttak/hobbycam/blob/main/PRIVACY.md`;
   - App Privacy → **Data Not Collected**;
   - Pricing and Availability → price **Free**;
   - age rating questionnaire: all "None";
   - screenshots 16:10 (1280×800, 1440×900, 2560×1600 or 2880×1800); the 1280×800 images in `docs/src/images/` qualify;
   - copyright "2026 Norbert Takacs";
   - review notes: "No account needed. Sample drawing: File → Import DXF…, docs/src/examples/bracket.dxf from
     https://github.com/norberttak/hobbycam";
   - export compliance is answered by `ITSAppUsesNonExemptEncryption = false` in Info.plist.
6. **Submit:** select the uploaded build, **Add for Review**, **Submit**.

## 8. GPL and the App Store

The Free Software Foundation's position is that the App Store terms add restrictions that are incompatible with GPL §10
(VLC was removed from the App Store in 2011 over this). As the **sole copyright holder** you may distribute your own code
on any terms, including through the App Store, so the App Store build is fine today: HobbyCAM contains no third-party
GPL/LGPL code, and the `cargo-about` licence gate (CI `licences` job, `about.toml`) fails the build if a GPL dependency is
ever added.

External contributions make their authors copyright holders too. The README says contributions are accepted under
GPL-3.0-or-later *and may also be distributed through app stores*. Keep that line, make sure contributors have seen it
(e.g. in the pull request template or a comment on their first PR), or stop App Store releases if you accept code under
other terms. Never add `GPL-*` or `LGPL-*` to `accepted` in `about.toml`.

## 9. Troubleshooting

- **Notarization "Invalid":** the job prints `notarytool log`; or run
  `xcrun notarytool log <submission-id> --key AuthKey.p8 --key-id <id> --issuer <issuer>` locally.
- **"The signature does not include a secure timestamp":** codesign ran without network access to Apple's timestamp
  server; re-run the job.
- **"The executable does not have the hardened runtime enabled":** the app was signed without `--options runtime`
  (`scripts/macos-dmg.sh` does this; `scripts/check-macos-bundle.sh` checks it).
- **Partial secrets error in the macOS job:** one of the six secrets is missing or misspelled; the error lists the names.
- **Transporter ITMS-90296** (app sandbox not enabled): the package was not built by `scripts/macos-appstore.sh`, or the
  entitlements were not applied; `codesign -d --entitlements - target/macos/HobbyCAM.app` must show
  `com.apple.security.app-sandbox`.
- **Transporter ITMS-90889** (provisioning profile missing): `APPSTORE_PROFILE` must be the *Mac App Store Connect* profile
  for `io.github.norberttak.hobbycam`.
- **SDK too old / minimum Xcode version:** Apple raises the minimum SDK for uploads; build with the current Xcode
  (`xcode-select -p`, `xcodebuild -version`).
- **hdiutil deprecation warnings** ("Please use 'diskutil image …'") on recent macOS: only warnings, the dmg is still
  built correctly.
- **Tag does not match the version:** the `check` job compares `vX.Y.Z` with `[workspace.package] version`. Delete the
  tag (`git tag -d vX.Y.Z; git push origin :refs/tags/vX.Y.Z`), fix, tag again.
- **Icon change:** edit `app/src/icon_gen.rs`, run `scripts/make-icons.sh`, commit `app/assets/icon/*`.
- **Licence gate fails on a new dependency:** check its licence; add permissive licences to `accepted` in `about.toml`
  (never GPL/LGPL, see section 8) or pick another crate.
